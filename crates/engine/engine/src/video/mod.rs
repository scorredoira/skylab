//! Recording the game as it is seen and heard, to a video to share (F12 starts and stops it):
//! the window's picture as the player sees it (the scene, the game's overlay) and what the game
//! plays, into an MP4 of H.264 and AAC in `~/Movies/<name>/`.
//!
//! Nothing waits for it. Each frame that is due (`Pacer`: 60 a second, whatever the screen's rate)
//! is copied on the GPU, shrunk to the video's size, and copied into one of a few buffers that are
//! read once the GPU is done with them; when none is free the frame is left out, and the video
//! shows the one before a little longer. The buffers are written into the file on a thread of their
//! own. What the recording itself shows (the red dot and the time, the notice of the file saved) is
//! drawn after the copy: it is not in the video.
//!
//! The file is made by the system's encoders, so where there are none (anywhere but macOS) the key
//! says so. With `ENGINE_CLIPS_DIR` set, the files go there instead; with `ENGINE_CLIP=<s>,<n>` a
//! clip of `n` seconds is recorded `s` seconds after the start and the game closes once it is saved
//! (to try it without a hand on the keyboard).

// Elsewhere the writer is never started: its messages go unused.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
mod mac;

use crate::audio::tap;
use crate::gpu::{Frame, Gpu};
use crate::Instant;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use winit::keyboard::KeyCode;

/// The key that starts and stops a recording.
pub const KEY: KeyCode = KeyCode::F12;
/// Frames a second of the video.
pub const FPS: u32 = 60;
/// The video is at most this wide and tall: the window's picture is shrunk to fit.
pub const LARGEST: (u32, u32) = (1920, 1080);
/// Buffers the frames are read back through: while all are busy, frames are left out.
const BUFFERS: usize = 8;
/// How long a notice stays on the screen, in seconds.
const NOTICE: f64 = 8.0;

/// Whether this system can record (it has the encoders).
pub const AVAILABLE: bool = cfg!(target_os = "macos");

/// What a video is made with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    /// Pixels, both even.
    pub size: (u32, u32),
    pub fps: u32,
    /// Bits a second of the picture.
    pub bits: u32,
    /// Samples a second of the sound, if there is sound.
    pub audio_rate: Option<u32>,
}

/// What a finished recording holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Frames written, and the same again where none came (the window hidden, the game slower
    /// than the video's rate).
    pub frames: u64,
    pub repeated: u64,
    /// Frames the encoder could not take, and frames not read back because every buffer was busy.
    pub dropped: u64,
    pub skipped: u64,
    pub seconds: f64,
}

/// The size of the video for a window of `width` x `height` pixels: as large as it is, or shrunk
/// to fit in `largest`, its shape kept; both even (the colour is stored for each two by two).
pub fn fit(width: u32, height: u32, largest: (u32, u32)) -> (u32, u32) {
    let (w, h) = (width.max(2) as f64, height.max(2) as f64);
    let scale = (largest.0 as f64 / w).min(largest.1 as f64 / h).min(1.0);
    let even = |x: f64| ((x * scale / 2.0).round() as u32 * 2).max(2);
    (even(w), even(h))
}

/// Bits a second for a video of this size and rate: enough for a moving landscape to stay sharp,
/// small enough to send (about 60 MB a minute at 1920 x 1080 and 60 frames).
pub fn bits(size: (u32, u32), fps: u32) -> u32 {
    let per_pixel = 0.07;
    ((size.0 as f64 * size.1 as f64 * fps as f64 * per_pixel) as u32).clamp(2_000_000, 9_000_000)
}

/// The name of a recording made at `time` (year, month, day, hour, minute, second), as the system
/// names its own: "Skylab 2026-11-18 at 14.03.22.mp4".
pub fn file_name(name: &str, time: [i32; 6]) -> String {
    let [y, mo, d, h, mi, s] = time;
    format!("{name} {y:04}-{mo:02}-{d:02} at {h:02}.{mi:02}.{s:02}.mp4")
}

/// A file in `dir` for a recording made now that is not there yet: " 2", " 3"... after the name
/// when two are made in the same second.
fn free_path(dir: &Path, name: &str, time: [i32; 6]) -> PathBuf {
    let first = dir.join(file_name(name, time));
    if !first.exists() {
        return first;
    }
    let stem = file_name(name, time).trim_end_matches(".mp4").to_string();
    (2..).map(|n| dir.join(format!("{stem} {n}.mp4"))).find(|p| !p.exists()).expect("a free name")
}

/// Where the recordings go: `ENGINE_CLIPS_DIR`, or `~/Movies/<name>`.
fn folder(name: &str) -> PathBuf {
    if let Some(dir) = std::env::var_os("ENGINE_CLIPS_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join("Movies").join(name)
}

/// Which frames go into the video: one for each 1/fps of a second, the first drawn in it. Its
/// times start at the first frame taken, so frames drawn at the video's own rate fall in the
/// middle of their slots, and the jitter of the screen's rate does not move them from one to the next.
#[derive(Clone, Debug)]
pub struct Pacer {
    fps: f64,
    start: Option<f64>,
    last: Option<u64>,
}

impl Pacer {
    pub fn new(fps: u32) -> Pacer {
        Pacer { fps: fps as f64, start: None, last: None }
    }

    /// The slot a frame drawn at `t` seconds would fill, or `None` when its slot has a frame.
    pub fn due(&self, t: f64) -> Option<u64> {
        let Some(start) = self.start else { return Some(0) };
        let slot = ((t - start) * self.fps).round().max(0.0) as u64;
        (self.last.is_none_or(|last| slot > last)).then_some(slot)
    }

    /// The frame drawn at `t` was taken for `slot` (from `due`).
    pub fn took(&mut self, t: f64, slot: u64) {
        self.start.get_or_insert(t);
        self.last = Some(slot);
    }
}

/// Where in the sound's samples (counted from the video's first frame) what the tap kept goes.
/// The tap's samples follow each other without gaps, so they are counted from the first, whose
/// time is when it was played; a new first (the tap lost some) starts the count again.
#[derive(Clone, Debug, Default)]
pub struct SoundClock {
    anchor: Option<(Instant, i64)>,
    count: i64,
}

impl SoundClock {
    /// For `frames` samples whose first, or an earlier one of the same run, was played at `start`:
    /// how many to leave out (they come before the video's start, `epoch`), and the sample the rest
    /// begin at.
    pub fn place(&mut self, start: Instant, epoch: Instant, rate: u32, frames: usize) -> (usize, i64) {
        if self.anchor.is_none_or(|(at, _)| at != start) {
            let offset = if start >= epoch { ((start - epoch).as_secs_f64() * rate as f64).round() as i64 } else { -(((epoch - start).as_secs_f64() * rate as f64).round() as i64) };
            self.anchor = Some((start, offset));
            self.count = 0;
        }
        let at = self.anchor.map_or(0, |(_, offset)| offset) + self.count;
        self.count += frames as i64;
        let skip = (-at).clamp(0, frames as i64) as usize;
        (skip, at + skip as i64)
    }
}

enum ToWriter {
    /// Buffer `index` holds the frame of `slot` (read back if `mapped`); the video's first frame was at `epoch`.
    Frame { index: usize, buffer: wgpu::Buffer, mapped: bool, slot: u64, epoch: Instant },
    /// No more frames: finish the file, with the video lasting until slot `end`.
    Stop { skipped: u64, end: u64 },
}

enum FromWriter {
    /// Buffer `index` can be used again.
    Free(usize),
    Done(Result<Summary, String>),
}

/// The GPU's side: the window's picture copied, shrunk to the video's size, and read back.
struct Readback {
    size: (u32, u32),
    /// Bytes from one row to the next in the buffers.
    stride: u32,
    format: wgpu::TextureFormat,
    small: wgpu::Texture,
    small_view: wgpu::TextureView,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The window's picture copied (it cannot be read by a shader), at its size, and its bind group.
    whole: Option<(wgpu::Texture, wgpu::BindGroup)>,
    buffers: Vec<wgpu::Buffer>,
    free: Vec<usize>,
}

impl Readback {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat, size: (u32, u32)) -> Readback {
        // BGRA, as the encoder takes it; sRGB when the window's picture is, so the bytes are the same.
        let small_format = if format.is_srgb() { wgpu::TextureFormat::Bgra8UnormSrgb } else { wgpu::TextureFormat::Bgra8Unorm };
        let small = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("video frame"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: small_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let small_view = small.create_view(&Default::default());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("video shrink"), source: wgpu::ShaderSource::Wgsl(include_str!("shrink.wgsl").into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video shrink"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("video shrink"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video shrink"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("fullscreen"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &shader, entry_point: Some("shrink"), compilation_options: Default::default(), targets: &[Some(small_format.into())] }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let stride = (size.0 * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffers = (0..BUFFERS)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("video readback"),
                    size: stride as u64 * size.1 as u64,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                })
            })
            .collect();
        Readback { size, stride, format, small, small_view, pipeline, layout, sampler, whole: None, buffers, free: (0..BUFFERS).collect() }
    }

    /// Records the copy of `picture` into a free buffer; its index, or `None` if none is free.
    fn encode(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, picture: &wgpu::Texture) -> Option<usize> {
        if picture.format() != self.format {
            return None;
        }
        let index = self.free.pop()?;
        let (w, h) = (picture.width(), picture.height());
        if self.whole.as_ref().is_none_or(|(t, _)| t.width() != w || t.height() != h) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("video picture"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video shrink"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
            });
            self.whole = Some((texture, group));
        }
        let (whole, group) = self.whole.as_ref().expect("made above");
        encoder.copy_texture_to_texture(picture.as_image_copy(), whole.as_image_copy(), wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video shrink"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.small_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // A window made another shape since the start keeps its shape, with black at the sides.
            let (vw, vh) = (self.size.0 as f32, self.size.1 as f32);
            let scale = (vw / w as f32).min(vh / h as f32);
            let (fw, fh) = (w as f32 * scale, h as f32 * scale);
            pass.set_viewport((vw - fw) / 2.0, (vh - fh) / 2.0, fw, fh, 0.0, 1.0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
        encoder.copy_texture_to_buffer(
            self.small.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &self.buffers[index], layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.stride), rows_per_image: Some(self.size.1) } },
            wgpu::Extent3d { width: self.size.0, height: self.size.1, depth_or_array_layers: 1 },
        );
        Some(index)
    }
}

/// A recording under way.
struct Recording {
    path: PathBuf,
    readback: Readback,
    pacer: Pacer,
    /// When the recording was started, and when its first frame was taken.
    began: Instant,
    epoch: Option<Instant>,
    to_writer: Sender<ToWriter>,
    from_writer: Receiver<FromWriter>,
    writer: Option<std::thread::JoinHandle<()>>,
    /// The frame recorded this frame, to read back once submitted: its buffer and slot.
    queued: Option<(usize, u64)>,
    skipped: u64,
    /// Since when no more frames are taken: the file is finished once the buffers are back.
    stopping: Option<Instant>,
    done: Option<Result<Summary, String>>,
}

impl Recording {
    fn start(gpu: &mut Gpu, name: &str) -> Result<Recording, String> {
        if !AVAILABLE {
            return Err("Recording the screen is not available on this system".into());
        }
        if !gpu.set_capture(true) {
            return Err("This window's picture cannot be recorded".into());
        }
        let dir = folder(name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = free_path(&dir, name, crate::clock::local_time().unwrap_or([1970, 1, 1, 0, 0, 0]));
        let size = fit(gpu.width, gpu.height, LARGEST);
        let settings = Settings { size, fps: FPS, bits: bits(size, FPS), audio_rate: tap::rate() };
        let readback = Readback::new(&gpu.device, gpu.color_format, size);
        let (to_writer, from_main) = channel();
        let (to_main, from_writer) = channel();
        let writer = spawn_writer(path.clone(), settings, readback.stride as usize, from_main, to_main)?;
        tap::start();
        Ok(Recording {
            path,
            readback,
            pacer: Pacer::new(FPS),
            began: Instant::now(),
            epoch: None,
            to_writer,
            from_writer,
            writer: Some(writer),
            queued: None,
            skipped: 0,
            stopping: None,
            done: None,
        })
    }

    fn seconds(&self) -> f64 {
        self.epoch.map_or(0.0, |e| (Instant::now() - e).as_secs_f64())
    }

    /// Before submitting a frame: its picture into a buffer, if it is due and one is free.
    fn capture(&mut self, gpu: &Gpu, frame: &mut Frame) {
        if self.stopping.is_some() {
            return;
        }
        let now = Instant::now();
        let t = (now - self.began).as_secs_f64();
        let Some(slot) = self.pacer.due(t) else { return };
        let picture = gpu.picture(frame).clone();
        match self.readback.encode(&gpu.device, &mut frame.encoder, &picture) {
            Some(index) => {
                self.pacer.took(t, slot);
                self.epoch.get_or_insert(now);
                self.queued = Some((index, slot));
            }
            None => self.skipped += 1,
        }
    }

    /// After the frame was submitted: its buffer is read when the GPU is done with it, and handed
    /// to the writer then (on the thread that polls the device: this one).
    fn submitted(&mut self) {
        let Some((index, slot)) = self.queued.take() else { return };
        let buffer = self.readback.buffers[index].clone();
        let (tx, epoch) = (self.to_writer.clone(), self.epoch.expect("set with the frame"));
        let sent = buffer.clone();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(ToWriter::Frame { index, buffer: sent, mapped: result.is_ok(), slot, epoch });
        });
    }

    /// Once a frame: buffers back from the writer, the end. True once the file is finished.
    fn poll(&mut self) -> bool {
        while let Ok(message) = self.from_writer.try_recv() {
            match message {
                FromWriter::Free(index) => self.readback.free.push(index),
                FromWriter::Done(result) => self.done = Some(result),
            }
        }
        if let Some(since) = self.stopping {
            // All the buffers back (their frames written), or a while waited for them.
            if (self.readback.free.len() == BUFFERS || since.elapsed() > Duration::from_secs(2)) && self.writer.is_some() {
                let _ = self.to_writer.send(ToWriter::Stop { skipped: self.skipped, end: self.end() });
                // The writer finishes the file on its own; its handle is kept for the end of the game.
                if let Some(w) = self.writer.take() {
                    FINISHING.with_borrow_mut(|f| f.push(w));
                }
            }
        }
        self.done.is_some()
    }

    /// The slot the video ends at: when it was stopped (now if it was not).
    fn end(&self) -> u64 {
        let Some(epoch) = self.epoch else { return 0 };
        let until = self.stopping.unwrap_or_else(Instant::now);
        ((until - epoch).as_secs_f64() * FPS as f64).round() as u64
    }

    fn stop(&mut self) {
        self.stopping.get_or_insert(Instant::now());
    }
}

thread_local! {
    /// The writers finishing their files: waited for when the game closes (`Recorder::drop`).
    static FINISHING: std::cell::RefCell<Vec<std::thread::JoinHandle<()>>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The thread that writes the file: frames as they come, the tap's sound between them.
#[cfg(target_os = "macos")]
fn spawn_writer(path: PathBuf, settings: Settings, stride: usize, from_main: Receiver<ToWriter>, to_main: Sender<FromWriter>) -> Result<std::thread::JoinHandle<()>, String> {
    std::thread::Builder::new()
        .name("video".into())
        .spawn(move || {
            let mut writer = match mac::Writer::new(&path, settings) {
                Ok(w) => Some(w),
                Err(e) => {
                    let _ = to_main.send(FromWriter::Done(Err(e)));
                    None
                }
            };
            let rate = settings.audio_rate.unwrap_or(48_000);
            let mut clock = SoundClock::default();
            let mut epoch = None;
            // Sound not written yet (the encoder busy, or the first frame not come): its samples and where they begin.
            let mut waiting: Vec<f32> = Vec::new();
            let mut waiting_at = 0;
            let mut skipped = 0;
            let mut end = 0;
            loop {
                let stop = match from_main.recv_timeout(Duration::from_millis(10)) {
                    Ok(ToWriter::Frame { index, buffer, mapped, slot, epoch: first }) => {
                        epoch = Some(first);
                        if mapped {
                            if let Some(w) = &mut writer {
                                if let Ok(view) = buffer.slice(..).get_mapped_range() {
                                    w.frame(&view, stride, slot);
                                }
                            }
                            buffer.unmap();
                        }
                        let _ = to_main.send(FromWriter::Free(index));
                        false
                    }
                    Ok(ToWriter::Stop { skipped: s, end: e }) => {
                        skipped = s;
                        end = e;
                        true
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => false,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => true,
                };
                let taken = tap::take();
                if let (Some(start), Some(epoch)) = (taken.start, epoch) {
                    let frames = taken.samples.len() / 2;
                    let (skip, at) = clock.place(start, epoch, rate, frames);
                    if waiting.is_empty() {
                        waiting_at = at;
                    }
                    waiting.extend_from_slice(&taken.samples[skip * 2..]);
                }
                if let Some(w) = &mut writer {
                    if !waiting.is_empty() && (w.audio_ready() || stop) {
                        w.audio(&waiting, waiting_at);
                        waiting.clear();
                    }
                }
                if stop {
                    break;
                }
            }
            tap::stop();
            if let Some(w) = writer {
                let result = w.finish(end).map(|summary| Summary { skipped, ..summary });
                let _ = to_main.send(FromWriter::Done(result));
            }
        })
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "macos"))]
fn spawn_writer(_: PathBuf, _: Settings, _: usize, _: Receiver<ToWriter>, _: Sender<FromWriter>) -> Result<std::thread::JoinHandle<()>, String> {
    Err("Recording the screen is not available on this system".into())
}

/// A line shown for a while at the bottom: a file saved (with a button to show it), or why not.
struct Notice {
    text: String,
    file: Option<PathBuf>,
    shown: Instant,
}

/// F12: records, stops, and says so on the screen (`ui`, drawn over the picture after it is taken).
pub(crate) struct Recorder {
    name: String,
    recording: Option<Recording>,
    notice: Option<Notice>,
    /// `ENGINE_CLIP`: seconds after the start, and how long.
    test: Option<(f64, f64)>,
    /// With `ENGINE_CLIP`: frames drawn in the seconds before the clip and during it, and their
    /// seconds, to say what recording costs.
    measured: [(u32, f64); 2],
    opened: Instant,
}

impl Recorder {
    pub(crate) fn new(name: &str) -> Recorder {
        let test = std::env::var("ENGINE_CLIP").ok().and_then(|v| {
            let (after, length) = v.split_once(',')?;
            Some((after.trim().parse().ok()?, length.trim().parse().ok()?))
        });
        Recorder { name: name.to_string(), recording: None, notice: None, test, measured: [(0, 0.0); 2], opened: Instant::now() }
    }

    /// F12: starts a recording, or stops the one under way.
    pub(crate) fn toggle(&mut self, gpu: &mut Gpu) {
        match &mut self.recording {
            Some(r) => r.stop(),
            None => match Recording::start(gpu, &self.name) {
                Ok(r) => {
                    self.notice = None;
                    self.recording = Some(r);
                }
                Err(e) => {
                    gpu.set_capture(false);
                    self.say(e, None);
                }
            },
        }
    }

    fn say(&mut self, text: String, file: Option<PathBuf>) {
        eprintln!("video: {text}");
        self.notice = Some(Notice { text, file, shown: Instant::now() });
    }

    /// Once a frame, before drawing. True when the game should close (`ENGINE_CLIP`'s clip is saved).
    pub(crate) fn poll(&mut self, gpu: &mut Gpu) -> bool {
        if let Some((after, length)) = self.test {
            let t = self.opened.elapsed().as_secs_f64();
            match &mut self.recording {
                None if t >= after && self.notice.is_none() => self.toggle(gpu),
                Some(r) if r.stopping.is_none() && r.began.elapsed().as_secs_f64() >= length => r.stop(),
                _ => {}
            }
        }
        if self.recording.is_some() {
            // Delivers the buffers read back (their callbacks), without waiting.
            let _ = gpu.device.poll(wgpu::PollType::Poll);
        }
        let Some(r) = &mut self.recording else { return false };
        if !r.poll() {
            return false;
        }
        let r = self.recording.take().expect("polled above");
        gpu.set_capture(false);
        match r.done.expect("done") {
            Ok(s) => {
                eprintln!(
                    "video: {} ({:.2} s, {} frames, {} repeated, {} dropped by the encoder, {} left out with the buffers busy)",
                    r.path.display(),
                    s.seconds,
                    s.frames,
                    s.repeated,
                    s.dropped,
                    s.skipped
                );
                let place = r.path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                let home = std::env::var("HOME").unwrap_or_default();
                let place = if !home.is_empty() && place.starts_with(&home) { format!("~{}", &place[home.len()..]) } else { place };
                let file = r.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                self.say(format!("Saved \"{file}\" in {place}"), Some(r.path.clone()));
            }
            Err(e) => self.say(format!("The recording could not be saved: {e}"), None),
        }
        if self.test.is_some() {
            let rate = |(n, s): (u32, f64)| if s > 0.0 { n as f64 / s } else { 0.0 };
            eprintln!("video: the game drew {:.1} frames a second before the clip, {:.1} while recording", rate(self.measured[0]), rate(self.measured[1]));
        }
        self.test.is_some()
    }

    /// Every frame drawn, before it is submitted: its picture taken if a recording wants it.
    pub(crate) fn capture(&mut self, gpu: &Gpu, frame: &mut Frame) {
        if let Some((after, length)) = self.test {
            let t = self.opened.elapsed().as_secs_f64();
            let during = self.recording.as_ref().is_some_and(|r| r.epoch.is_some() && r.stopping.is_none());
            let window = if during { Some(1) } else { (self.recording.is_none() && t >= after - length && t < after).then_some(0) };
            if let Some(w) = window {
                self.measured[w].0 += 1;
                self.measured[w].1 = if w == 0 { t - (after - length) } else { self.recording.as_ref().map_or(0.0, |r| r.seconds()) };
            }
        }
        if let Some(r) = &mut self.recording {
            r.capture(gpu, frame);
        }
    }

    pub(crate) fn submitted(&mut self) {
        if let Some(r) = &mut self.recording {
            r.submitted();
        }
    }

    /// Whether there is anything to draw (`ui`).
    pub(crate) fn shows(&mut self) -> bool {
        if self.notice.as_ref().is_some_and(|n| n.shown.elapsed().as_secs_f64() > NOTICE) {
            self.notice = None;
        }
        self.recording.is_some() || self.notice.is_some()
    }

    /// The red dot and the time while recording; the notice. Not in the video.
    pub(crate) fn ui(&mut self, ctx: &egui::Context) {
        use egui::{vec2, Align2, Color32, FontId};
        let screen = ctx.content_rect();
        if let Some(r) = &self.recording {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("video-recording")));
            let label = if r.stopping.is_some() {
                "Saving…".to_string()
            } else {
                let s = r.seconds() as u64;
                format!("{}:{:02}", s / 60, s % 60)
            };
            let centre = egui::pos2(screen.center().x, screen.top() + 22.0);
            let font = FontId::monospace(15.0);
            let width = painter.layout_no_wrap(label.clone(), font.clone(), Color32::WHITE).size().x + 40.0;
            painter.rect_filled(egui::Rect::from_center_size(centre, vec2(width, 28.0)), 14.0, Color32::from_black_alpha(160));
            // The dot blinks slowly, as a camera's does.
            let on = r.stopping.is_some() || (r.seconds() * 1.0).fract() < 0.7;
            let dot = egui::pos2(centre.x - width / 2.0 + 15.0, centre.y);
            painter.circle_filled(dot, 6.0, if on { Color32::from_rgb(230, 40, 40) } else { Color32::from_rgb(110, 30, 30) });
            painter.text(egui::pos2(dot.x + 13.0, centre.y), Align2::LEFT_CENTER, label, font, Color32::WHITE);
        }
        let mut reveal = None;
        if let Some(n) = &self.notice {
            egui::Area::new(egui::Id::unique("video-notice")).order(egui::Order::Foreground).anchor(Align2::CENTER_BOTTOM, vec2(0.0, -24.0)).show(ctx, |ui| {
                egui::Frame::new().fill(Color32::from_black_alpha(200)).corner_radius(8.0).inner_margin(10.0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&n.text).color(Color32::WHITE));
                        if cfg!(target_os = "macos") && n.file.is_some() && ui.button("Show in Finder").clicked() {
                            reveal = n.file.clone();
                        }
                    });
                });
            });
        }
        if let Some(file) = reveal {
            let _ = std::process::Command::new("open").arg("-R").arg(file).spawn();
            self.notice = None;
        }
    }
}

impl Drop for Recorder {
    /// The game closes: a recording under way is finished (its file closed) before it does.
    fn drop(&mut self) {
        if let Some(mut r) = self.recording.take() {
            let _ = r.to_writer.send(ToWriter::Stop { skipped: r.skipped, end: r.end() });
            if let Some(w) = r.writer.take() {
                FINISHING.with_borrow_mut(|f| f.push(w));
            }
        }
        for writer in FINISHING.take() {
            let started = Instant::now();
            while !writer.is_finished() && started.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_video_fits_in_full_hd_with_even_sides() {
        // A Retina window of 1440 x 900 points.
        assert_eq!(fit(2880, 1800, LARGEST), (1728, 1080));
        assert_eq!(fit(1600, 1000, LARGEST), (1600, 1000));
        // Wide: the width is what limits.
        assert_eq!(fit(3840, 1080, LARGEST), (1920, 540));
        // Odd sides are made even.
        assert_eq!(fit(1001, 777, LARGEST), (1002, 778));
        let (w, h) = fit(3024, 1890, LARGEST);
        assert!(w <= 1920 && h <= 1080 && w % 2 == 0 && h % 2 == 0, "{w}x{h}");
    }

    #[test]
    fn the_bit_rate_grows_with_the_size_and_stays_shareable() {
        let full = bits((1920, 1080), 60);
        assert!((6_000_000..=9_000_000).contains(&full), "{full}");
        assert!(bits((640, 360), 30) == 2_000_000);
        assert!(bits((1728, 1080), 60) < full);
    }

    #[test]
    fn files_are_named_by_the_time_and_never_overwrite() {
        let time = [2026, 11, 8, 9, 3, 7];
        assert_eq!(file_name("Skylab", time), "Skylab 2026-11-08 at 09.03.07.mp4");
        let dir = std::env::temp_dir().join(format!("engine-video-names-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = free_path(&dir, "Skylab", time);
        assert_eq!(first, dir.join("Skylab 2026-11-08 at 09.03.07.mp4"));
        std::fs::write(&first, b"").unwrap();
        let second = free_path(&dir, "Skylab", time);
        assert_eq!(second, dir.join("Skylab 2026-11-08 at 09.03.07 2.mp4"));
        std::fs::write(&second, b"").unwrap();
        assert_eq!(free_path(&dir, "Skylab", time), dir.join("Skylab 2026-11-08 at 09.03.07 3.mp4"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Frames drawn at `hz` (with some jitter) for a second: the slots they fill.
    fn paced(hz: f64, fps: u32, jitter: f64) -> Vec<u64> {
        let mut pacer = Pacer::new(fps);
        let mut slots = Vec::new();
        for i in 0..(hz as usize) {
            // A fixed pattern of jitter, up to `jitter` of a frame either way.
            let wobble = [0.0, 0.7, -0.4, 0.2, -0.9, 0.5][i % 6] * jitter / hz;
            let t = 3.0 + i as f64 / hz + wobble;
            if let Some(slot) = pacer.due(t) {
                pacer.took(t, slot);
                slots.push(slot);
            }
        }
        slots
    }

    #[test]
    fn a_screen_at_the_videos_rate_fills_every_slot_once() {
        let slots = paced(60.0, 60, 0.3);
        assert_eq!(slots, (0..60).collect::<Vec<_>>());
    }

    #[test]
    fn a_faster_screen_gives_one_frame_a_slot() {
        // 120 Hz (ProMotion): every other frame, none skipped.
        let slots = paced(120.0, 60, 0.2);
        // 60 a second (the last frame of the second may round to the next one's first slot).
        assert!((60..=61).contains(&slots.len()), "{slots:?}");
        assert!(slots.windows(2).all(|w| w[1] == w[0] + 1), "{slots:?}");
        // 144 Hz: each slot has its frame.
        let slots = paced(144.0, 60, 0.0);
        assert!(slots.windows(2).all(|w| w[1] == w[0] + 1), "{slots:?}");
    }

    #[test]
    fn a_slower_game_leaves_gaps_where_the_last_frame_stays() {
        // 45 frames a second into 60: every frame taken, a slot left empty after every third.
        let slots = paced(45.0, 60, 0.0);
        assert_eq!(slots.len(), 45);
        let gaps = slots.windows(2).filter(|w| w[1] == w[0] + 2).count();
        assert_eq!(gaps, 15, "{slots:?}");
        assert!(slots.windows(2).all(|w| w[1] > w[0] && w[1] <= w[0] + 2));
    }

    #[test]
    fn a_frame_left_out_lets_the_next_fill_its_slot() {
        let mut pacer = Pacer::new(60);
        let t0 = 10.0;
        pacer.took(t0, pacer.due(t0).unwrap());
        // No buffer for the next one: not taken, so the one after (120 Hz) takes slot 1.
        assert_eq!(pacer.due(t0 + 1.0 / 60.0), Some(1));
        assert_eq!(pacer.due(t0 + 1.5 / 60.0), Some(2));
        pacer.took(t0 + 1.4 / 60.0, 1);
        assert_eq!(pacer.due(t0 + 1.45 / 60.0), None);
    }

    #[test]
    fn the_sound_follows_on_from_its_first_sample() {
        let epoch = Instant::now();
        let mut clock = SoundClock::default();
        // The first block played 10 ms after the video's first frame, at 48 kHz.
        let start = epoch + Duration::from_millis(10);
        assert_eq!(clock.place(start, epoch, 48_000, 480), (0, 480));
        // The next blocks of the same run follow without a gap, whatever their clock says.
        assert_eq!(clock.place(start, epoch, 48_000, 512), (0, 960));
        assert_eq!(clock.place(start, epoch, 48_000, 100), (0, 1472));
        // A new run (the tap lost some) is placed by its own time.
        let later = epoch + Duration::from_secs(2);
        assert_eq!(clock.place(later, epoch, 48_000, 10), (0, 96_000));
    }

    #[test]
    fn sound_from_before_the_first_frame_is_left_out() {
        let start = Instant::now();
        let epoch = start + Duration::from_millis(5);
        let mut clock = SoundClock::default();
        // 5 ms before: 240 samples at 48 kHz.
        assert_eq!(clock.place(start, epoch, 48_000, 480), (240, 0));
        assert_eq!(clock.place(start, epoch, 48_000, 480), (0, 240));
        // A block wholly before the start is all left out.
        let mut clock = SoundClock::default();
        assert_eq!(clock.place(start, epoch, 48_000, 100).0, 100);
    }
}
