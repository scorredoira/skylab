# engine

Reusable pieces for 3D games. It knows nothing about any particular game: no aircraft, no terrain.
A game implements one trait and the engine runs it, in a window or straight to a PNG.

Check it works: `cargo run -p engine --example smoke -- out.png` (writes an image) or without
arguments (opens a window). `examples/smoke.rs` is also the shortest complete example of using it.
`cargo run --release -p engine --example compare -- a.png b.png` counts the pixels in which two
pictures differ. `--example shrink -- in.png out.jpg <width> [<quality>] [<x>,<y>,<w>,<h>]` cuts
and shrinks a picture into a JPEG (a game's pictures to ship, such as the flight manual's).

## What a game implements

```rust
pub trait Game {
    fn update(&mut self, input: &Input, dt: f64, gpu: &Gpu);      // advance and prepare buffers
    fn ui(&mut self, ctx: &egui::Context) {}                      // 2D overlay
    fn draw(&mut self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>); // 3D scene
    fn samples(&self) -> u32 { 4 }                                // samples per pixel wanted: 4 or 1
    fn post(&self) -> Post { Post::default() }                    // exposure, adaptation, bloom, contrast, colour, the lens
    fn clip_name(&self) -> Option<&str> { None }                  // the recordings' name (F12); the window's title if None
    fn translucent(&self) -> bool { false }                       // a second pass this frame?
    fn before_translucent(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {} // between the passes
    fn draw_translucent(&mut self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>) {}    // the second pass
}
engine::run(title, width, height, |gpu| MyGame::new(gpu));
engine::render_to_png(path, width, height, scale, |gpu| MyGame::new(gpu), |game, frames| done);
```

Each frame calls `update`, then `draw` inside the scene pass, then makes the picture from the scene
as `post` says, then draws `ui` on top. `render_to_png`
does the same without a window and is how anything visual is tested.

**A second pass, for what is see-through and soft.** When `translucent` says so (asked after
`update`), the scene pass keeps its colour and depth, the game may record work of its own that reads
the depth (`before_translucent`: an effect drawn into targets of its own at a lower resolution), and
`draw_translucent` draws in a second pass over the same colour, with the depth tested but not
written, so its shaders can read it at the same time (`Gpu::scene_depth`): smoke and fire that
meet the ground without a seam. Keeping the scene costs a little, so say yes only when there is
something to draw in it; otherwise the frame is one pass, as before. Pipelines drawn there must not
write depth. Bind groups of `scene_depth` are made again when `Gpu::targets_made` changes.
When the game also says `reads_scene`, the first pass leaves its colour, resolved, in a picture
the second can read (`Gpu::scene_kept`; only with several samples per pixel): for what is drawn
over the scene out of the scene itself, a wet road that mirrors what stands on it or glass with
drops that bend what is behind them.

## Modules

| Module | What it gives |
|---|---|
| `app` | `Game`, `run`, `render_to_png`: the loop, window events, overlay. |
| `gpu` | `Gpu`: device, queue, formats, the multisampled HDR scene pass and the second (`translucent_pass`, `scene_depth`, `targets_made`), the post-processing pass, the overlay pass, `read_pixels`. |
| `post` | `Post`: how the scene becomes the picture (exposure in stops, how much it adapts to the scene and to its middle, bloom, contrast, saturation, and what a lens adds: vignette, a blur of speed, the sun's flare, depth of field); the passes in `post.wgsl` and `focus.wgsl`. |
| `pipeline` | `Gpu::shader`, `Gpu::shader_with_ending_loops` (faster, for sources whose loops all end; `pipeline::shader_with_ending_loops` the same with only the device, to make a pipeline on another thread), `Gpu::pipeline(&PipelineDesc)`: a render pipeline for the scene pass in one call, opaque or blended "over"; `Gpu::pipeline_blended` with a blend of its own (the sea's surface, which lets through part of what is under it and covers part); `Gpu::pipeline_with_samples` for a picture of the scene's formats drawn before it with its own samples (the sea's mirror). |
| `mesh` | `MeshBuilder`: flat-shaded triangles from boxes, lofted rings and fans; smooth-shaded cylinders and `skin`s through rings, made as fine as their curves need (within `SAG`, the same for every model), with creases where they fold (`Skin`, and any edge sharper than `CREASE_ANGLE`); `Mesh` on the GPU, as it is (`build`) or with each distinct vertex once and the triangles as `u32` indices (`build_indexed`, drawn with `draw_indexed`; for meshes drawn many times over). |
| `camera` | `Camera`: `f64` position, `f32` orientation, reversed-depth projection, optional vertical shift. |
| `canvas` | `Canvas`: an egui picture painted into a texture of its own (`paint`, then bind `view()` as a `texture_2d<f32>`), for screens and signs in the scene. Each paint makes every mip level after it; read it with `Gpu::texture_sampler` (trilinear, anisotropic) and it stays sharp however small or slanted it is seen. |
| `occlusion` | `SkyVolume::bake`: how much sky each point inside a mesh sees through its openings, baked on every core into a volume (`Gpu::volume`, `texture_3d`); a surface facing `n` reads `r + dot(gba - 0.5, n)`. `ambient_occlusion`: how open each point of a surface is to what is round it, for the occlusion of the light that comes from everywhere. |
| `input` | `Input`: keys held, keys pressed this frame, pointer, buttons, whether the overlay has the pointer, the game controllers. |
| `controller` | `Controller`: one gamepad or joystick as plain data; `ControllerPort`: reads them (gilrs). |
| `audio` | `AudioOutput::start(fill)`: a callback that fills each block of samples. On the web it starts at the first key or click (browsers play nothing before). `Clip::from_wav`: a recording (PCM of 16 or 24 bits, float of 32, mixed to mono) and its rate; `wav_bytes`: samples written as a WAV file. What the newest output plays is also kept while a recording wants it (`tap`, for `video`). |
| `video` | F12 in any game's window: a video of what is seen and heard (see "Recording a video"). `fit`, `bits`, `file_name`, `Pacer`, `SoundClock`: its sizes, names and timing, tested on their own. |
| `uniforms` | `UniformArena`: one small uniform block per draw call, in a single buffer with dynamic offsets. |
| `threads` | `threads::spawn` (a background job), `threads::scope` (data split over the cores), `threads::wait` (a result from a channel) and `threads::lock` (a lock shared with a job): std's threads natively; on the web, Web Workers (see "On the web"). Use these, not `std::thread`, in code that runs in the game. |
| `files` | `files::read`, `files::exists`: the game's own files (textures), from disk natively and from what the page downloaded on the web; `files::load_text`, `files::save_text`: the player's (settings), on disk or in the browser's `localStorage`. |
| `program` | Not on the web: `program::beside` (another program of the same build, in the folder of the running executable) and `program::hand_over` (it is started in this one's place, which ends): one game opening another. |
| `web` | Only on the web: the page's canvas, the loading line (`say`, `ready`), the game's files downloaded from a list (`download`), panics shown on the page. |
| `texture` | `Gpu::texture`, `Gpu::texture_array`, `prepare_layers` and `Gpu::upload_layers` (the same in two steps, the first off the GPU), `Gpu::texture_sampler`, `Gpu::volume` (a 3D texture of RGBA data): images as textures with CPU-built mip chains; layout entry helpers; `decode` and `decode_bytes`: a PNG or JPEG, from a file or from memory, as RGBA. |

`engine::egui`, `engine::wgpu` and `engine::KeyCode` are re-exported so a game needs no versions of its own.
`engine::Instant` is the clock for real time: std's natively, the browser's on the web, where `std::time::Instant::now` panics.

## Controllers

`Input::controllers` is the list of connected gamepads and joysticks, brought up to date before each
`update`; devices come and go at any time. A game without a window makes its own
`ControllerPort::new()` and calls `poll(&mut list)`.

```rust
pub struct Controller { pub id: usize, pub name: String, pub standard: bool, pub axes: Vec<AxisState>, pub buttons: Vec<ButtonState> }
pub struct AxisState { pub index: u32, pub name: Option<PadAxis>, pub value: f32 }      // raw() = as the device sends it
pub struct ButtonState { pub index: u32, pub name: Option<PadButton>, pub down: bool, pub pressed: bool, pub value: f32 }
controller.axis(PadAxis::LeftStickY)        // -1...1, up positive; 0 if not reported yet
controller.axis_raw(1)                      // Option: -1...1 as the device sends it (stick back = positive)
controller.is_down(PadButton::South)  .pressed(..)  .button_value(PadButton::RightTrigger2)  // 0...1
controller.button_raw(3)                    // Option<&ButtonState>, numbered from 1
```

- **By name** (`PadAxis`, `PadButton`: gilrs's `Axis` and `Button`) for gamepads: `standard` says gilrs
  has the device in its layout database, so South is the bottom face button on every one of them.
- **By number** for everything else (flight sticks, throttles, pedals): axes 0 X, 1 Y, 2 Z, 3 Rx,
  4 Ry, 5 Rz, 6 slider, 7 dial, 8 wheel, 16 and 17 the hat; buttons from 1 as HID numbers them. The
  numbers are the same on macOS and Linux for an ordinary HID device. Such a device still gets a few
  names from gilrs, which mean nothing: ignore them.
- An axis or button is in the list from the first time the device reports it; an axis that has
  not moved since it was plugged in is not there yet (`axis_raw` gives `None`).
- A hat (`DPadX`, `DPadY`) also holds down `DPadLeft`, `DPadRight`, `DPadUp`, `DPadDown`, so the
  directions are in one place whether the device has a hat or four buttons.
- No dead zone or filtering is applied: shaping is the game's.
- `pad_axis_named("LeftStickX")` and `pad_button_named("South")` read the names `{:?}` prints, for
  mapping files.

## Conventions a game must follow

- **Camera-relative drawing.** `Camera::view_proj` has no translation: subtract `camera.position`
  from every object's position (in `f64`) and draw the `f32` difference. That is what keeps a world
  of thousands of kilometres from shaking.
- **Reversed depth.** 1 at the near plane, 0 at infinity. The scene pass clears depth to 0;
  pipelines made with `Gpu::pipeline` already test with `GreaterEqual`.
- **Mesh vertices** are 10 floats: position, normal, colour with gloss in alpha (`MeshBuilder::layout()`).
- **Shaders write linear HDR light.** The scene pass is `Rgba16Float` (`Gpu::scene_format`): write
  linear colour, as bright as it is (a sunlit wall near 1, the sun's disc far above), with no tone
  mapping, no clamp and no gamma. The engine exposes and tone-maps the whole frame once, at the end
  (see below). Alpha blending happens in linear light.
- **The overlay** is drawn after the tone mapping, straight into the sRGB target: egui's colours are
  final.
- **Units in the overlay** are logical points, not pixels.
- **Samples.** The scene pass has `Gpu::sample_count` samples per pixel, 4 unless `Game::samples`
  says 1. The engine asks before every `update`; when the count changes, pipelines made with
  `Gpu::pipeline` before are for the old count and must be made again (compare `gpu.sample_count`
  with the one they were made for).

## From light to picture (`Post`)

After `draw` the engine makes the final image from the scene in three steps (`post.rs`, `post.wgsl`),
and what a lens adds when a game asks for it (below; all off by default, at no cost):

1. **Bloom.** The scene is halved six times (Jimenez 2014's filter of thirteen bilinear taps, which
   does not flicker when a small light moves) and added back up through the dual filter's eight
   taps (Bjorge 2015): a soft glow round what is bright, wide and physical, with no threshold. `Post::bloom` is
   the part of the light spread into it (some 3 %); 0 skips those passes (unless the exposure adapts, which measures on them), for a low quality setting.
2. **Exposure** in stops: `Post::exposure` for a scene whose average is mid grey (0.18). With
   `Post::adaptation` above 0 the exposure follows the scene's average brightness (measured on the
   smallest bloom level), smoothly: about 1 s to open up for a darker scene, 0.5 s to close down.
   With `Post::spot` above 0 it also opens for what the eye looks at: that share of the stops the
   middle of the picture (a soft spot a third of it wide) would be left more than 1.5 under mid
   grey, up to 3, as the eye in a room opens for the wall it looks at beside a window; a middle
   bright enough changes nothing.
   With `Post::glare` above 0 it also stops down for a glare: the brightest patch of the smallest
   bloom level (a big glow, not a point) is kept within 4 stops above mid grey by that share
   (a patch counts by its weight: the edges less, and with a spot what is far from the middle hardly).
   `Gpu::exposure()` reads back the exposure used, for tests and tuning.
3. **AgX** tone mapping (Troy Sobotka's picture formation: its inset primaries, the logarithm of
   16.5 stops and an S-shaped curve through mid grey at the middle of the picture, written here as
   t / (1 + t^3)^(1/3) either side of it), then a dither of half a level against banding. Plain AgX
   is soft and pale; `Post::contrast` narrows the stops it spans round mid grey (0.4: 10 below and
   6.5 above become 7 and 4.6, the toe stays smooth) and `Post::saturation` scales colour round the
   luminance.

**The lens** (each 0 by default: nothing drawn, nothing paid):

- `Post::focus` (with `focus_span`, `focus_blur`, `near`): a depth of field, before the bloom
  (`focus.wgsl`). What is nearer or farther than the distance in focus, beyond the span that stays
  sharp, is blurred into a circle that grows with how far off it is for its distance, up to
  `focus_blur` (a fraction of the picture's height). One pass gathers 36 taps on a spiral, each
  counted by whether its own circle reaches the pixel (what is behind does not spread over what is
  sharp in front of it) and the less the brighter it is, so a glint does not speckle. The scene pass
  keeps its depth for it (`Gpu::scene_pass_keeping`, `Post::wants_depth`). 2.3 ms at 1600 x 1000 on an
  Apple M5 Pro.
- `Post::blur`, `blur_centre`, `blur_above`: a blur of speed. Each point is smeared along the line to
  the centre by `blur` of its distance from it (eight taps, dithered), from nothing near the centre:
  what the camera follows stays sharp. `blur_above` is the share of it for what is above the centre
  (the sky and the distance do not rush by as the ground does).
- `Post::flare`, `flare_sun`: the flare of a lens towards the sun: a glow, a streak and five faint
  ghosts along the line through the middle, as strong as the sun shows where it is (read on the
  bloom: behind a hill or a cloud it goes). Needs the bloom.
- `Post::warmth`: the white balance, towards amber or blue, before the tone mapping (0: the scene's).
- `Post::vignette`: the corners darker, after the tone mapping.

`tests/post.rs` checks it on the GPU through `render_to_png`.

## Textures

```rust
let files: Vec<TextureSource> = names.iter().map(|n| TextureSource::file(format!("assets/textures/{n}_color.jpg"))).collect();
let colour = gpu.texture_array("colour", &files, Kind::Color)?;   // texture_2d_array<f32>, one layer per file
let sampler = gpu.texture_sampler();                              // repeat, trilinear, anisotropic 16x
// bind group layout: texture::array_layout_entry(1), texture::sampler_layout_entry(2)
// bind group:        colour.entry(1), and the sampler
```

| `Kind` | GPU format | Mip levels are averaged |
|---|---|---|
| `Color` | `Rgba8UnormSrgb` | in linear light |
| `Normal` | `Rg8Unorm`: x and y, the shader rebuilds z | as stored |
| `NormalPacked` | `Rgba8Unorm`: normal x, y + two data channels (roughness, occlusion) | as stored |
| `Gray` | `R8Unorm`, from the red channel | as stored |
| `Data` | `Rgba8Unorm` | as stored |

- A `TextureSource` is a PNG or JPEG file, three files packed into one image (`Channels`, for
  `NormalPacked`) or RGBA pixels in memory. All the layers of an array must have the same size.
- `Gpu::texture` gives a single `texture_2d<f32>` (`texture::layout_entry`). Both return a
  `Texture`; `Texture::bytes()` is the GPU memory it takes (1024 x 1024 RGBA with mips: 5.3 MB per layer).
- Reading the files and building the mip chains is the slow part: `texture::prepare_layers` does
  it without a GPU, on another thread (`threads::spawn`), and `Gpu::upload_layers` only copies the
  result, so a game can load textures while it draws.
- Normal maps follow the OpenGL convention: red = +u, green = up the image, blue = out of the surface.
- No block compression: textures are uploaded as 8-bit RGBA, RG or R.
- `examples/textured.rs` draws a ground and buildings from two arrays, with world-space and
  triplanar mapping, normal mapping and two blended scales against repetition:
  `cargo run -p engine --example textured -- out.png [grazing|above|objects|domes]`.
- `examples/prepare_textures.rs` shrinks downloaded PBR materials to a common size and draws
  contact sheets of candidates; `assets/textures/fetch.sh` uses it. Its `web` makes the smaller copies the web
  build downloads (`scripts/build-web.sh`).

## On the web

The same game runs in a browser with WebGPU (`wasm32-unknown-unknown`; a game's page and build:
`flight`'s `web/` and `scripts/build-web.sh`). What changes, and where:

- **Start.** `run` returns at once: the browser runs the loop (winit's `spawn_app`). The window is
  the page's `<canvas id="game">` (`web::window_attributes`), sized by the page; the GPU is set up
  asynchronously and `make` is called once it is ready, so it must own what it needs (`'static`).
  `render_to_png` is native only.
- **Threads.** Web Workers sharing the module's memory: the module is built with wasm's atomics
  (`scripts/build-web.sh`), and the page must be served cross-origin isolated (COOP and COEP
  headers). The page's thread may never wait: a contended lock or a `recv` there is a trap that
  ends the game. So `spawn` hands jobs to a pool of workers (`threads::start_workers` starts some
  before anything waits for them: a worker only starts once the page's thread has gone back to the
  browser); `scope` runs its jobs one after another; `wait` and `lock` spin on the page's thread.
  A lock the page's thread shares with a job is taken with `threads::lock`, or held only for a
  moment and taken with `try_lock` in a loop (`sim_core::Terrain`'s caches). wgpu's objects are not
  `Send` there: they stay on the page's thread.
- **Files.** The game's own are downloaded before it starts from a list (`web::download`, then
  `files::read` by the same relative paths); the player's (`files::load_text`, `save_text`) are kept
  in `localStorage`.
- **Sound** (`audio`) is WebAudio through cpal, opened at the first key or click. **Controllers**
  are the browser's Gamepad API through gilrs (a pad shows once one of its buttons is pressed).
- **Panics** are shown on the page (`web::show_panics`) and in the console.

## Recording a video

`F12` in the window of any game starts recording it and `F12` again saves the video; the game never
sees the key. The video is the window's picture as the player sees it, the scene and the game's
overlay, and what the game plays (every `AudioOutput` is tapped: `audio::tap`), into
`~/Movies/<name>/<name> <date> at <time>.mp4` (`Game::clip_name`, else the window's title). While
it records, a red dot and the time are drawn at the top, and when it is saved a notice says where,
with a button that shows the file in the Finder; both are drawn after the picture is taken, so
neither is in the video.

- **The file**: H.264 (High, 4:2:0, BT.709) at 60 frames a second, the picture shrunk to fit in
  1920 x 1080 (its sides even), some 7 to 9 Mbit/s (`bits`); AAC of two channels at the output's
  rate, 160 kbit/s; MP4 with its index at the front. What phones, WhatsApp and X play as it is.
  It is made by the system's own encoders (AVFoundation's `AVAssetWriter`; H.264 in hardware), so
  nothing has to be installed; there is no encoder elsewhere, and on Linux, Windows and the web
  the key says that recording is not available there.
- **Nothing waits for it.** A frame is taken when its 1/60 s slot has none yet (`Pacer`, whose
  slots start at the first frame taken, so a screen at 60 Hz fills each once and one at 120 Hz
  every other). It is copied on the GPU (the window's picture can be copied, not read by a
  shader: `Gpu::set_capture` lets it be copied while recording), shrunk with four blended reads
  a pixel (`shrink.wgsl`) and copied into one of eight buffers, read once the GPU is done with it
  (the device is polled without waiting each frame). The file is written on a thread of its own.
  When every buffer is busy, or the encoder is, the frame is left out; a slot without a frame (a
  game slower than 60, a hidden window) shows the one before, so the video keeps real time and
  the sound stays with it. The sound's samples follow each other from the first played after the
  first frame (`SoundClock`), and the video ends when it was stopped.
- **To try it**: `ENGINE_CLIPS_DIR=<dir>` writes the videos there instead; `ENGINE_CLIP=<s>,<n>`
  records `n` seconds starting `s` seconds after the window opens, prints what was written (frames,
  frames repeated or left out, and the frames a second the game drew before and while recording)
  and closes the game once the file is saved, e.g.
  `ENGINE_CLIPS_DIR=/tmp/clips ENGINE_CLIP=25,5 scripts/drive.sh --driver`.

## Not here yet

Shadows, compute helpers and model loading. They go in when a game needs them (see the rules
in `ARCHITECTURE.md`). Particles and a fire simulated on the GPU exist, as the world's
(`world-render`), on the second pass and with `wgpu` directly.
