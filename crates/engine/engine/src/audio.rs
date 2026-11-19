//! Sound output: the game supplies a function that fills each block of samples. Recordings read
//! from WAV files (`Clip`), and samples written as one (`wav_bytes`).
//!
//! What is played can also be recorded (`tap`): the screen recording (`video`) takes its sound there.
//!
//! Web: a browser plays no sound before the player has touched the page, so the output (WebAudio,
//! through cpal) is opened at the first key or click (`app` calls `gesture`), and is silent till then.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Keeps the output stream alive; dropping it stops the sound.
pub struct AudioOutput {
    #[cfg(not(target_arch = "wasm32"))]
    _stream: cpal::Stream,
    #[cfg(target_arch = "wasm32")]
    _stream: std::rc::Rc<std::cell::RefCell<Option<cpal::Stream>>>,
}

type Fill = Box<dyn FnMut(&mut [f32], usize, f32) + Send + 'static>;

/// Opens the default output and plays `fill` on it.
fn open(mut fill: Fill) -> Option<cpal::Stream> {
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels as usize;
    let rate = config.sample_rate as f32;
    let source = tap::source(rate as u32);
    let stream = device
        .build_output_stream(
            config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                fill(data, channels, rate);
                tap::push(source, data, channels);
            },
            |err| eprintln!("audio: {err}"),
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}

impl AudioOutput {
    /// Starts playing. `fill(samples, channels, sample_rate)` is called on the audio thread with an
    /// interleaved buffer to fill. Returns `None` if there is no output device.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(fill: impl FnMut(&mut [f32], usize, f32) + Send + 'static) -> Option<AudioOutput> {
        Some(AudioOutput { _stream: open(Box::new(fill))? })
    }

    /// Web: opened at the first gesture (see the module); `None` if the browser has no WebAudio.
    #[cfg(target_arch = "wasm32")]
    pub fn start(fill: impl FnMut(&mut [f32], usize, f32) + Send + 'static) -> Option<AudioOutput> {
        cpal::default_host().default_output_device()?;
        let stream = std::rc::Rc::new(std::cell::RefCell::new(None));
        let slot = std::rc::Rc::downgrade(&stream);
        let fill: Fill = Box::new(fill);
        web::WAITING.with_borrow_mut(|waiting| {
            waiting.push(Box::new(move || {
                // Not if the game dropped its output meanwhile.
                if let Some(slot) = slot.upgrade() {
                    *slot.borrow_mut() = open(fill);
                }
            }))
        });
        Some(AudioOutput { _stream: stream })
    }
}

/// A recorded sound: its samples (mono, −1 to 1) and the rate they were recorded at.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub samples: Vec<f32>,
    pub rate: f32,
}

impl Clip {
    /// A WAV file's sound: PCM of 16 or 24 bits or float of 32, any number of channels (mixed to one).
    pub fn from_wav(bytes: &[u8]) -> Result<Clip, String> {
        let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return Err("not a WAV file".into());
        }
        let (mut format, mut channels, mut rate, mut bits, mut data) = (0, 0, 0, 0, None);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let size = u32_at(bytes, at + 4) as usize;
            let body = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            match &bytes[at..at + 4] {
                b"fmt " if body.len() >= 16 => {
                    format = u16_at(body, 0);
                    channels = u16_at(body, 2) as usize;
                    rate = u32_at(body, 4);
                    bits = u16_at(body, 14);
                    // WAVE_FORMAT_EXTENSIBLE: the real format is the first two bytes of its GUID.
                    if format == 0xFFFE && body.len() >= 26 {
                        format = u16_at(body, 24);
                    }
                }
                b"data" => data = Some(body),
                _ => {}
            }
            at += 8 + size + (size & 1);
        }
        let data = data.ok_or("no data in the WAV file")?;
        if channels == 0 || rate == 0 {
            return Err("no format in the WAV file".into());
        }
        let one: Box<dyn Fn(&[u8]) -> f32> = match (format, bits) {
            (1, 16) => Box::new(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
            (1, 24) => Box::new(|b| (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8388608.0),
            (3, 32) => Box::new(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => return Err(format!("WAV format {format} with {bits} bits is not read")),
        };
        let width = bits as usize / 8;
        let samples = data.chunks_exact(width * channels).map(|frame| frame.chunks_exact(width).map(&one).sum::<f32>() / channels as f32).collect();
        Ok(Clip { samples, rate: rate as f32 })
    }
}

/// Interleaved samples (−1 to 1) as a WAV file of 16-bit PCM.
pub fn wav_bytes(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// What the output plays, kept while a recording wants it. The newest output opened is the one
/// kept (a game that opens another stops hearing the old one's), always as two channels. Only
/// macOS records (`video`): elsewhere nothing takes what is kept.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) mod tap {
    use crate::Instant;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;

    /// The samples kept since the last `take`: interleaved, two channels.
    #[derive(Default)]
    pub(crate) struct Taken {
        /// When the first of them was played (handed to the output), if any were.
        pub start: Option<Instant>,
        pub samples: Vec<f32>,
    }

    struct Kept {
        /// The output whose samples are kept, and its samples per second.
        source: u64,
        rate: u32,
        /// When the first sample since `start` was played.
        first: Option<Instant>,
        samples: Vec<f32>,
    }

    static ON: AtomicBool = AtomicBool::new(false);
    static OUTPUTS: AtomicU64 = AtomicU64::new(0);
    static KEPT: Mutex<Kept> = Mutex::new(Kept { source: 0, rate: 0, first: None, samples: Vec::new() });

    /// At most this many seconds wait to be taken; older are lost (a recording that stopped taking).
    const MOST: u32 = 10;

    /// A new output, playing `rate` samples a second: its number, for `push`. It is the one kept now.
    pub(crate) fn source(rate: u32) -> u64 {
        let n = OUTPUTS.fetch_add(1, Ordering::Relaxed) + 1;
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        kept.source = n;
        kept.rate = rate;
        n
    }

    /// The samples per second of the output kept, if one was ever opened.
    pub(crate) fn rate() -> Option<u32> {
        let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        (kept.source != 0).then_some(kept.rate)
    }

    /// From now on what is played is kept (`take`), from empty.
    pub(crate) fn start() {
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        kept.first = None;
        kept.samples.clear();
        ON.store(true, Ordering::Release);
    }

    /// Nothing more is kept, and what was is dropped.
    pub(crate) fn stop() {
        ON.store(false, Ordering::Release);
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        kept.first = None;
        kept.samples = Vec::new();
    }

    /// What was played since the last `take`.
    pub(crate) fn take() -> Taken {
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        let start = kept.first.take();
        Taken { start, samples: std::mem::take(&mut kept.samples) }
    }

    /// On the audio thread, after `source`'s block of interleaved samples was filled.
    pub(crate) fn push(source: u64, data: &[f32], channels: usize) {
        if !ON.load(Ordering::Acquire) {
            return;
        }
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        if kept.source != source {
            return;
        }
        if kept.first.is_none() {
            kept.first = Some(Instant::now());
        }
        let most = (kept.rate * 2 * MOST) as usize;
        if kept.samples.len() > most {
            kept.samples.clear();
            kept.first = Some(Instant::now());
        }
        stereo(data, channels, &mut kept.samples);
    }

    /// Interleaved samples of any number of channels, appended to `out` as two: one channel is
    /// played on both, more than two keep the first two (the front left and right).
    pub(crate) fn stereo(data: &[f32], channels: usize, out: &mut Vec<f32>) {
        match channels {
            0 => {}
            1 => out.extend(data.iter().flat_map(|&s| [s, s])),
            2 => out.extend_from_slice(data),
            n => out.extend(data.chunks_exact(n).flat_map(|frame| [frame[0], frame[1]])),
        }
    }
}

/// Web: the player touched the page (a key, a click): the outputs waiting for it are opened.
#[cfg(target_arch = "wasm32")]
pub fn gesture() {
    for open in web::WAITING.take() {
        open();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_written_reads_back() {
        let samples: Vec<f32> = (0..200).map(|i| ((i as f32) * 0.1).sin() * 0.5).collect();
        // Two channels, the same in both: read back as one.
        let stereo: Vec<f32> = samples.iter().flat_map(|&s| [s, s]).collect();
        let clip = Clip::from_wav(&wav_bytes(&stereo, 2, 44100)).unwrap();
        assert_eq!(clip.rate, 44100.0);
        assert_eq!(clip.samples.len(), 200);
        for (a, b) in clip.samples.iter().zip(&samples) {
            assert!((a - b).abs() < 1e-4);
        }
        assert!(Clip::from_wav(b"RIFF....WAVEjunk").is_err());
    }

    #[test]
    fn any_channels_are_recorded_as_two() {
        let mut out = Vec::new();
        tap::stereo(&[0.1, 0.2], 1, &mut out);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2]);
        out.clear();
        tap::stereo(&[0.1, 0.2, 0.3, 0.4], 2, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
        out.clear();
        // Six channels (5.1): the front left and right of each frame.
        tap::stereo(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0], 6, &mut out);
        assert_eq!(out, [1.0, 2.0, 7.0, 8.0]);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    thread_local! {
        /// What opens each output started before the first gesture.
        pub static WAITING: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    }
}
