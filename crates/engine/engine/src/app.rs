//! The main loop: a window, a game, and a 2D overlay on top.

use crate::{ControllerPort, Gpu, Input, Instant, Post};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{ModifiersState, PhysicalKey};
use winit::window::{Window, WindowId};

/// What a game implements to run in the engine's loop. Each frame calls `update`, `ui`, then `draw`.
pub trait Game {
    /// Advance the game by `dt` seconds of real time and prepare what will be drawn.
    fn update(&mut self, input: &Input, dt: f64, gpu: &Gpu);
    /// Describe the 2D overlay: instruments, text, menus.
    fn ui(&mut self, _ctx: &egui::Context) {}
    /// Draw the 3D scene into the scene pass.
    fn draw(&mut self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>);
    /// Whether this frame has a second pass, `draw_translucent`. Asked after `update`; say yes only
    /// when there is something to draw in it: keeping the scene for it costs a little.
    fn translucent(&self) -> bool {
        false
    }
    /// Draw what is see-through and must be soft where it meets the scene (smoke, fire), in a second
    /// pass whose shaders can read the depth of the first (`Gpu::scene_depth`). Only when `translucent`.
    /// Work between the two passes, when there is a second one: what reads the scene's depth into
    /// targets of its own (an effect drawn at a lower resolution, then put over the scene in the second pass).
    fn before_translucent(&mut self, _gpu: &Gpu, _encoder: &mut wgpu::CommandEncoder) {}
    fn draw_translucent(&mut self, _gpu: &Gpu, _pass: &mut wgpu::RenderPass<'_>) {}
    /// Whether the second pass reads the colour the first left (`Gpu::scene_kept`): a wet road
    /// that mirrors what stands on it, say. Asked after `update`, when `translucent`; keeping the
    /// picture costs a resolve of the scene.
    fn reads_scene(&self) -> bool {
        false
    }
    /// Samples per pixel the scene pass should have: 4, or 1 for no smoothing of edges. Asked before
    /// every `update`; when the answer changes, `Gpu::sample_count` changes and the game must make
    /// its pipelines again.
    fn samples(&self) -> u32 {
        Gpu::SAMPLES
    }
    /// How the scene, drawn in linear light, becomes the picture: exposure, its adaptation, bloom.
    /// Asked every frame, after `draw`.
    fn post(&self) -> Post {
        Post::default()
    }
    /// The name of the game's recordings (F12, `video`): their folder in `~/Movies` and the start
    /// of each file's name. The window's title when `None`.
    fn clip_name(&self) -> Option<&str> {
        None
    }
}

struct Overlay {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
}

impl Overlay {
    fn new(gpu: &Gpu) -> Overlay {
        Overlay { ctx: egui::Context::default(), renderer: egui_wgpu::Renderer::new(&gpu.device, gpu.color_format, egui_wgpu::RendererOptions::default()) }
    }

    /// Runs the game's `ui` and draws the result over the frame.
    fn paint<G: Game>(&mut self, game: &mut G, gpu: &Gpu, frame: &mut crate::gpu::Frame, raw: egui::RawInput, scale: f32) -> egui::PlatformOutput {
        self.paint_with(gpu, frame, raw, scale, |ui| game.ui(ui))
    }

    /// Runs `ui` and draws the result over the frame.
    fn paint_with(&mut self, gpu: &Gpu, frame: &mut crate::gpu::Frame, raw: egui::RawInput, scale: f32, ui: impl FnMut(&mut egui::Ui)) -> egui::PlatformOutput {
        let output = self.ctx.run_ui(raw, ui);
        let jobs = self.ctx.tessellate(output.shapes, scale);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [gpu.width, gpu.height], pixels_per_point: scale };
        let mut textures = output.textures_delta;
        for (id, deltas) in std::mem::take(&mut textures.set) {
            for delta in deltas.iter() {
                self.renderer.update_texture(&gpu.device, &gpu.queue, id, delta);
            }
        }
        self.renderer.update_buffers(&gpu.device, &gpu.queue, &mut frame.encoder, &jobs, &screen);
        {
            let mut pass = gpu.overlay_pass(frame).forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        for id in std::mem::take(&mut textures.free) {
            self.renderer.free_texture(&id);
        }
        output.platform_output
    }
}

struct Running<G: Game> {
    window: Arc<Window>,
    gpu: Gpu,
    game: G,
    overlay: Overlay,
    events: egui_winit::State,
    input: Input,
    controllers: ControllerPort,
    last: Instant,
    /// The recording of the window (F12), and what it shows over the picture once it is taken.
    video: crate::video::Recorder,
    notes: Overlay,
}

struct Handler<G: Game, F: FnOnce(&Gpu) -> G> {
    title: String,
    #[cfg(not(target_arch = "wasm32"))]
    size: (u32, u32),
    make: Option<F>,
    running: Option<Running<G>>,
    #[cfg(target_arch = "wasm32")]
    starting: Option<web::Starting>,
}

impl<G: Game, F: FnOnce(&Gpu) -> G> Handler<G, F> {
    /// Makes the game once the window has its GPU.
    fn start(&mut self, window: Arc<Window>, mut gpu: Gpu) {
        // The window may have changed size while the GPU was being set up (the web's canvas does).
        let size = window.inner_size();
        gpu.resize(size.width, size.height);
        let game = (self.make.take().expect("started twice"))(&gpu);
        let overlay = Overlay::new(&gpu);
        let notes = Overlay::new(&gpu);
        let events = egui_winit::State::new(overlay.ctx.clone(), egui::ViewportId::ROOT, &window, Some(window.scale_factor() as f32), None, None);
        window.request_redraw();
        let video = crate::video::Recorder::new(game.clip_name().unwrap_or(&self.title));
        self.running = Some(Running { window, gpu, game, overlay, events, input: Input::default(), controllers: ControllerPort::new(), last: Instant::now(), video, notes });
    }
}

impl<G: Game, F: FnOnce(&Gpu) -> G> ApplicationHandler for Handler<G, F> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.make.is_none() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let attrs = Window::default_attributes()
                .with_title(&self.title)
                .with_inner_size(winit::dpi::LogicalSize::new(self.size.0, self.size.1))
                .with_window_icon(crate::icon::window_icon());
            let window = Arc::new(event_loop.create_window(attrs).expect("no window"));
            #[cfg(target_os = "macos")]
            crate::icon::dock();
            let size = window.inner_size();
            let gpu = block_on(Gpu::for_window(window.clone(), size.width, size.height));
            self.start(window, gpu);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(starting) = &mut self.starting {
            starting.begin(event_loop, &self.title);
        }
    }

    /// Web: the GPU is ready (`web::Starting`).
    #[cfg(target_arch = "wasm32")]
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _ready: ()) {
        if let Some((window, gpu)) = self.starting.as_mut().and_then(web::Starting::ready) {
            self.starting = None;
            self.start(window, gpu);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return };
        let response = r.events.on_window_event(&r.window, &event);
        let scale = r.window.scale_factor() as f32;
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => r.gpu.resize(size.width, size.height),
            WindowEvent::Focused(focused) => {
                r.input.unfocused = !focused;
                if !focused {
                    r.input.release_all();
                }
            }
            WindowEvent::ModifiersChanged(m) => r.input.shift = m.state().contains(ModifiersState::SHIFT),
            WindowEvent::KeyboardInput { event, .. } => {
                #[cfg(target_arch = "wasm32")]
                crate::audio::gesture();
                if let PhysicalKey::Code(crate::video::KEY) = event.physical_key {
                    // The recording's key is the engine's: the game never sees it.
                    if event.state == ElementState::Pressed && !event.repeat {
                        r.video.toggle(&mut r.gpu);
                    }
                } else if let PhysicalKey::Code(code) = event.physical_key {
                    // A key released always counts, so nothing stays stuck after typing in a menu. Only
                    // typing in a text field keeps a key from the game: a button or slider that took
                    // the focus (the arrows move it between them) must not swallow the game's keys.
                    let pressed = event.state == ElementState::Pressed;
                    if !pressed || !r.overlay.ctx.text_edit_focused() {
                        r.input.key(code, pressed);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = (position.x as f32 / scale, position.y as f32 / scale);
                r.input.mouse_delta.0 += p.0 - r.input.mouse.0;
                r.input.mouse_delta.1 += p.1 - r.input.mouse.1;
                r.input.mouse = p;
            }
            WindowEvent::MouseWheel { delta, .. } if !response.consumed => {
                match delta {
                    MouseScrollDelta::LineDelta(x, y) => r.input.wheel(x, y, false),
                    MouseScrollDelta::PixelDelta(p) => r.input.wheel(p.x as f32 / scale, p.y as f32 / scale, true),
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                #[cfg(target_arch = "wasm32")]
                crate::audio::gesture();
                // A click on the recording's notice is not the game's.
                let down = state == ElementState::Pressed && !response.consumed && !r.notes.ctx.is_pointer_over_egui();
                match button {
                    MouseButton::Left => {
                        r.input.clicked |= down;
                        r.input.left_button = down;
                    }
                    MouseButton::Right => r.input.right_button = down,
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = (now - r.last).as_secs_f64().min(0.25);
                r.last = now;
                r.input.pointer_over_ui = r.overlay.ctx.egui_wants_pointer_input();
                r.controllers.poll(&mut r.input.controllers);
                r.gpu.set_samples(r.game.samples());
                r.game.update(&r.input, dt, &r.gpu);
                r.input.end_frame();
                r.gpu.keep_scene(r.game.translucent() && r.game.reads_scene());
                if r.video.poll(&mut r.gpu) {
                    event_loop.exit();
                }
                if let Some(mut frame) = r.gpu.begin() {
                    draw_scene(&mut r.game, &r.gpu, &mut frame);
                    r.gpu.post_pass(&mut frame, &r.game.post(), dt);
                    let raw = r.events.take_egui_input(&r.window);
                    let notes_raw = r.video.shows().then(|| raw.clone());
                    let out = r.overlay.paint(&mut r.game, &r.gpu, &mut frame, raw, scale);
                    r.events.handle_platform_output(&r.window, out);
                    // The picture as the player sees it is taken here, before the recording's own marks.
                    r.video.capture(&r.gpu, &mut frame);
                    if let Some(raw) = notes_raw {
                        let video = &mut r.video;
                        r.notes.paint_with(&r.gpu, &mut frame, raw, scale, |ui| video.ui(ui));
                    }
                    r.gpu.finish(frame);
                    r.video.submitted();
                } else {
                    // Nothing waits for the screen while the window is hidden: without a pause the
                    // loop runs thousands of frames a second.
                    #[cfg(not(target_arch = "wasm32"))]
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                r.window.request_redraw();
            }
            _ => {}
        }
    }
}

/// The scene of a frame: `Game::draw`, then `Game::draw_translucent` if the game wants it.
fn draw_scene<G: Game>(game: &mut G, gpu: &Gpu, frame: &mut crate::gpu::Frame) {
    let translucent = game.translucent();
    {
        let mut pass = gpu.scene_pass_keeping(frame, translucent, game.post().wants_depth());
        game.draw(gpu, &mut pass);
    }
    if translucent {
        game.before_translucent(gpu, &mut frame.encoder);
        let mut pass = gpu.translucent_pass(frame);
        game.draw_translucent(gpu, &mut pass);
    }
}

/// Opens a window and runs the game until it is closed. `make` builds the game once the GPU exists.
#[cfg(not(target_arch = "wasm32"))]
pub fn run<G: Game>(title: &str, width: u32, height: u32, make: impl FnOnce(&Gpu) -> G) {
    let event_loop = EventLoop::new().expect("no event loop");
    let mut handler = Handler { title: title.to_string(), size: (width, height), make: Some(make), running: None };
    event_loop.run_app(&mut handler).expect("event loop failed");
}

/// Web: runs the game in the page's canvas, whose size is the page's (`width` and `height` are not
/// used). The browser runs the loop: this returns at once, and `make` is called later, once the GPU
/// is set up, so it owns what it needs.
#[cfg(target_arch = "wasm32")]
pub fn run<G: Game + 'static>(title: &str, width: u32, height: u32, make: impl FnOnce(&Gpu) -> G + 'static) {
    use winit::platform::web::EventLoopExtWebSys;
    let _ = (width, height);
    let event_loop = EventLoop::new().expect("no event loop");
    let starting = Some(web::Starting::new(event_loop.create_proxy()));
    event_loop.spawn_app(Handler { title: title.to_string(), make: Some(make), running: None, starting });
}

/// A GPU without a window and the overlay over it: frames drawn on demand and read back, for
/// pictures and video. The caller updates the game; `draw` only draws it.
#[cfg(not(target_arch = "wasm32"))]
pub struct Offscreen {
    pub gpu: Gpu,
    overlay: Overlay,
    /// Pixels to a point of the overlay.
    scale: f32,
}

#[cfg(not(target_arch = "wasm32"))]
impl Offscreen {
    pub fn new(width: u32, height: u32, scale: f32) -> Offscreen {
        let gpu = block_on(Gpu::headless(width, height));
        let overlay = Overlay::new(&gpu);
        Offscreen { gpu, overlay, scale }
    }

    /// Draws a frame of `game` as it stands, `dt` seconds after the last (for the exposure's
    /// adaptation), with its overlay, and submits it. Returns the milliseconds the scene, the
    /// overlay and the submission took on the CPU.
    pub fn draw<G: Game>(&mut self, game: &mut G, dt: f64) -> [f64; 3] {
        let t1 = Instant::now();
        self.gpu.keep_scene(game.translucent() && game.reads_scene());
        let mut frame = self.gpu.begin().expect("no frame");
        draw_scene(game, &self.gpu, &mut frame);
        self.gpu.post_pass(&mut frame, &game.post(), dt);
        let t2 = Instant::now();
        let (width, height) = (self.gpu.width as f32, self.gpu.height as f32);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width / self.scale, height / self.scale))), ..Default::default() };
        self.overlay.ctx.set_pixels_per_point(self.scale);
        self.overlay.paint(game, &self.gpu, &mut frame, raw, self.scale);
        let t3 = Instant::now();
        self.gpu.finish(frame);
        let t4 = Instant::now();
        [t2 - t1, t3 - t2, t4 - t3].map(|d| d.as_secs_f64() * 1000.0)
    }
}

/// Renders the game to a PNG without a window. Frames are drawn until `ready(game, frames_so_far)`
/// says the picture is complete (for games that load things in the background).
#[cfg(not(target_arch = "wasm32"))]
pub fn render_to_png<G: Game>(path: &str, width: u32, height: u32, scale: f32, make: impl FnOnce(&Gpu) -> G, mut ready: impl FnMut(&mut G, usize) -> bool) -> G {
    let mut screen = Offscreen::new(width, height, scale);
    let mut game = make(&screen.gpu);
    let input = Input::default();
    let mut frames = 0;
    // With ENGINE_PROFILE set, prints where the time of the last frames went.
    let profile = std::env::var_os("ENGINE_PROFILE").is_some();
    let mut spent = [0.0f64; 5];
    loop {
        screen.gpu.set_samples(game.samples());
        let t0 = Instant::now();
        game.update(&input, 1.0 / 60.0, &screen.gpu);
        let t1 = Instant::now();
        let [scene, overlay, submit] = screen.draw(&mut game, 1.0 / 60.0);
        let t4 = Instant::now();
        screen.gpu.wait();
        let t5 = Instant::now();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        for (s, d) in spent.iter_mut().zip([ms(t1 - t0), scene, overlay, submit, ms(t5 - t4)]) {
            *s = *s * 0.95 + d * 0.05;
        }
        frames += 1;
        if ready(&mut game, frames) {
            break;
        }
    }
    if profile {
        eprintln!("ms per frame: update {:.2}, scene {:.2}, overlay {:.2}, submit {:.2}, gpu wait {:.2}", spent[0], spent[1], spent[2], spent[3], spent[4]);
        eprintln!("exposure: {:+.2} stops", screen.gpu.exposure());
    }
    let pixels = screen.gpu.read_pixels();
    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8).expect("could not write the image");
    game
}

/// Waits for the GPU to be set up.
#[cfg(not(target_arch = "wasm32"))]
fn block_on<T>(setup: impl std::future::Future<Output = T>) -> T {
    pollster::block_on(setup)
}

/// Web: the window, in the page's canvas, and its GPU, which is set up asynchronously as the
/// browser's thread may never block. Once it is ready the loop gets a user event (`user_event`),
/// and the game is made then.
#[cfg(target_arch = "wasm32")]
mod web {
    use crate::Gpu;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::Arc;
    use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
    use winit::window::Window;

    pub(super) struct Starting {
        proxy: EventLoopProxy<()>,
        window: Option<Arc<Window>>,
        gpu: Rc<RefCell<Option<Gpu>>>,
    }

    impl Starting {
        pub(super) fn new(proxy: EventLoopProxy<()>) -> Starting {
            Starting { proxy, window: None, gpu: Rc::default() }
        }

        /// Makes the window and starts setting its GPU up, once.
        pub(super) fn begin(&mut self, event_loop: &ActiveEventLoop, title: &str) {
            if self.window.is_some() {
                return;
            }
            let window = Arc::new(event_loop.create_window(crate::web::window_attributes(title)).expect("no window"));
            let (slot, for_gpu, proxy) = (self.gpu.clone(), window.clone(), self.proxy.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let size = for_gpu.inner_size();
                *slot.borrow_mut() = Some(Gpu::for_window(for_gpu, size.width, size.height).await);
                let _ = proxy.send_event(());
            });
            self.window = Some(window);
        }

        /// The window and its GPU, once the GPU is set up.
        pub(super) fn ready(&mut self) -> Option<(Arc<Window>, Gpu)> {
            let gpu = self.gpu.borrow_mut().take()?;
            Some((self.window.take()?, gpu))
        }
    }
}
