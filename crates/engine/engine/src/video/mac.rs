//! The video file on macOS: AVFoundation's writer (`AVAssetWriter`) with the system's own encoders,
//! H.264 (in hardware where there is one) and AAC, into an MP4 whose index is at the front.

use super::{Settings, Summary};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::AnyObject;
use objc2::Message;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoColorPrimariesKey, AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPropertiesKey, AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey, AVVideoTransferFunctionKey, AVVideoTransferFunction_ITU_R_709_2, AVVideoWidthKey, AVVideoYCbCrMatrixKey, AVVideoYCbCrMatrix_ITU_R_709_2,
};
use objc2_avf_audio::{AVEncoderBitRateKey, AVFormatIDKey, AVNumberOfChannelsKey, AVSampleRateKey};
use objc2_core_audio_types::{kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatMPEG4AAC, AudioStreamBasicDescription};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::{kCMBlockBufferAssureMemoryNowFlag, CMAudioFormatDescriptionCreate, CMAudioSampleBufferCreateReadyWithPacketDescriptions, CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMTime};
use objc2_core_video::{kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA, CVPixelBuffer, CVPixelBufferLockFlags, CVPixelBufferPool};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use std::path::Path;
use std::ptr::{null, null_mut, NonNull};

/// The sound's bits a second, in AAC: plenty for two channels of a game.
const AUDIO_BITS: i32 = 160_000;

fn constant(c: Option<&'static NSString>) -> &'static NSString {
    c.expect("AVFoundation without one of its constants")
}

fn any<T: Message>(object: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an `AnyObject`.
    unsafe { Retained::cast_unchecked(object) }
}

fn dictionary(entries: &[(&NSString, Retained<AnyObject>)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = entries.iter().map(|(k, _)| *k).collect();
    let values: Vec<Retained<AnyObject>> = entries.iter().map(|(_, v)| v.clone()).collect();
    NSDictionary::from_retained_objects(&keys, &values)
}

fn int(n: i64) -> Retained<AnyObject> {
    any(NSNumber::numberWithLongLong(n))
}

fn cf_key(key: &CFString) -> &NSString {
    // SAFETY: a CFString is toll-free bridged to NSString.
    unsafe { &*(key as *const CFString as *const NSString) }
}

/// One file being written. Made, fed and finished on one thread (the recording's).
pub(super) struct Writer {
    writer: Retained<AVAssetWriter>,
    video: Retained<AVAssetWriterInput>,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    audio: Option<(Retained<AVAssetWriterInput>, CFRetained<CMFormatDescription>)>,
    settings: Settings,
    /// The last frame written, and its slot: again in the slots no frame came for.
    last: Option<(CFRetained<CVPixelBuffer>, u64)>,
    summary: Summary,
}

impl Writer {
    pub(super) fn new(path: &Path, settings: Settings) -> Result<Writer, String> {
        autoreleasepool(|_| unsafe { Self::make(path, settings) })
    }

    unsafe fn make(path: &Path, settings: Settings) -> Result<Writer, String> {
        let url = NSURL::from_file_path(path).ok_or("not a path to a file")?;
        let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, constant(AVFileTypeMPEG4)).map_err(|e| e.localizedDescription().to_string())?;
        // The index at the front: a player (a phone's, a web page's) starts before it has the whole file.
        writer.setShouldOptimizeForNetworkUse(true);

        let (width, height) = settings.size;
        let colour = dictionary(&[
            (constant(AVVideoColorPrimariesKey), any(constant(AVVideoColorPrimaries_ITU_R_709_2).retain())),
            (constant(AVVideoTransferFunctionKey), any(constant(AVVideoTransferFunction_ITU_R_709_2).retain())),
            (constant(AVVideoYCbCrMatrixKey), any(constant(AVVideoYCbCrMatrix_ITU_R_709_2).retain())),
        ]);
        let compression = dictionary(&[
            (constant(AVVideoAverageBitRateKey), int(settings.bits as i64)),
            (constant(AVVideoProfileLevelKey), any(constant(AVVideoProfileLevelH264HighAutoLevel).retain())),
            (constant(AVVideoMaxKeyFrameIntervalKey), int(settings.fps as i64 * 2)),
            (constant(AVVideoExpectedSourceFrameRateKey), int(settings.fps as i64)),
        ]);
        let output = dictionary(&[
            (constant(AVVideoCodecKey), any(constant(AVVideoCodecTypeH264).retain())),
            (constant(AVVideoWidthKey), int(width as i64)),
            (constant(AVVideoHeightKey), int(height as i64)),
            (constant(AVVideoColorPropertiesKey), any(colour)),
            (constant(AVVideoCompressionPropertiesKey), any(compression)),
        ]);
        let video = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(constant(AVMediaTypeVideo), Some(&output));
        video.setExpectsMediaDataInRealTime(true);
        let source = dictionary(&[
            (cf_key(kCVPixelBufferPixelFormatTypeKey), int(kCVPixelFormatType_32BGRA as i64)),
            (cf_key(kCVPixelBufferWidthKey), int(width as i64)),
            (cf_key(kCVPixelBufferHeightKey), int(height as i64)),
        ]);
        let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&video, Some(&source));
        if !writer.canAddInput(&video) {
            return Err("the video cannot be written".into());
        }
        writer.addInput(&video);

        let audio = match settings.audio_rate {
            Some(rate) => {
                let output = dictionary(&[
                    (constant(AVFormatIDKey), int(kAudioFormatMPEG4AAC as i64)),
                    (constant(AVSampleRateKey), int(rate as i64)),
                    (constant(AVNumberOfChannelsKey), int(2)),
                    (constant(AVEncoderBitRateKey), int(AUDIO_BITS as i64)),
                ]);
                let input = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(constant(AVMediaTypeAudio), Some(&output));
                input.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(&input) {
                    return Err("the sound cannot be written".into());
                }
                writer.addInput(&input);
                // What the game plays: 32-bit floats, two channels interleaved.
                let pcm = AudioStreamBasicDescription {
                    mSampleRate: rate as f64,
                    mFormatID: kAudioFormatLinearPCM,
                    mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
                    mBytesPerPacket: 8,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 8,
                    mChannelsPerFrame: 2,
                    mBitsPerChannel: 32,
                    mReserved: 0,
                };
                let mut format: *const CMFormatDescription = null();
                let status = CMAudioFormatDescriptionCreate(None, NonNull::from(&pcm), 0, null(), 0, null(), None, NonNull::from(&mut format));
                if status != 0 || format.is_null() {
                    return Err(format!("the sound's format ({status})"));
                }
                Some((input, CFRetained::from_raw(NonNull::new_unchecked(format as *mut CMFormatDescription))))
            }
            None => None,
        };

        if !writer.startWriting() {
            return Err(writer.error().map_or("the file cannot be written".into(), |e| e.localizedDescription().to_string()));
        }
        writer.startSessionAtSourceTime(CMTime::new(0, settings.fps as i32));
        Ok(Writer { writer, video, adaptor, audio, settings, last: None, summary: Summary::default() })
    }

    fn failed(&self) -> bool {
        unsafe { self.writer.status() == AVAssetWriterStatus::Failed }
    }

    /// A frame for `slot` (its time: `slot / fps` seconds): `rows` of BGRA pixels, `stride` bytes
    /// apart. The slots since the last frame that had none get the last frame again.
    pub(super) fn frame(&mut self, pixels: &[u8], stride: usize, slot: u64) {
        autoreleasepool(|_| unsafe {
            if self.failed() {
                return;
            }
            if let Some((last, at)) = &self.last {
                for gap in at + 1..slot {
                    if self.video.isReadyForMoreMediaData() && self.adaptor.appendPixelBuffer_withPresentationTime(last, CMTime::new(gap as i64, self.settings.fps as i32)) {
                        self.summary.repeated += 1;
                    } else {
                        self.summary.dropped += 1;
                    }
                }
            }
            let Some(buffer) = self.pixel_buffer() else {
                self.summary.dropped += 1;
                return;
            };
            let (width, height) = (self.settings.size.0 as usize, self.settings.size.1 as usize);
            if objc2_core_video::CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) != 0 {
                self.summary.dropped += 1;
                return;
            }
            let base = objc2_core_video::CVPixelBufferGetBaseAddress(&buffer) as *mut u8;
            let row = objc2_core_video::CVPixelBufferGetBytesPerRow(&buffer);
            for y in 0..height {
                let from = &pixels[y * stride..y * stride + width * 4];
                std::ptr::copy_nonoverlapping(from.as_ptr(), base.add(y * row), width * 4);
            }
            objc2_core_video::CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0));
            if self.video.isReadyForMoreMediaData() && self.adaptor.appendPixelBuffer_withPresentationTime(&buffer, CMTime::new(slot as i64, self.settings.fps as i32)) {
                self.summary.frames += 1;
            } else {
                self.summary.dropped += 1;
            }
            self.last = Some((buffer, slot));
        })
    }

    unsafe fn pixel_buffer(&self) -> Option<CFRetained<CVPixelBuffer>> {
        let pool = self.adaptor.pixelBufferPool()?;
        let mut out: *mut CVPixelBuffer = null_mut();
        if CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut out)) != 0 {
            return None;
        }
        Some(CFRetained::from_raw(NonNull::new(out)?))
    }

    /// Whether the sound can take more now; when not, the caller keeps it for later.
    pub(super) fn audio_ready(&self) -> bool {
        self.audio.as_ref().is_some_and(|(input, _)| unsafe { input.isReadyForMoreMediaData() })
    }

    /// Sound: interleaved pairs of samples, the first played at `at` (in samples since the start).
    pub(super) fn audio(&mut self, samples: &[f32], at: i64) {
        let Some((input, format)) = &self.audio else { return };
        let frames = samples.len() / 2;
        if frames == 0 || self.failed() {
            return;
        }
        autoreleasepool(|_| unsafe {
            let bytes = frames * 8;
            let mut block: *mut CMBlockBuffer = null_mut();
            if CMBlockBuffer::create_with_memory_block(None, null_mut(), bytes, None, null(), 0, bytes, kCMBlockBufferAssureMemoryNowFlag, NonNull::from(&mut block)) != 0 || block.is_null() {
                return;
            }
            let block = CFRetained::from_raw(NonNull::new_unchecked(block));
            if CMBlockBuffer::replace_data_bytes(NonNull::new_unchecked(samples.as_ptr() as *mut _), &block, 0, bytes) != 0 {
                return;
            }
            let mut sample: *mut CMSampleBuffer = null_mut();
            let rate = self.settings.audio_rate.unwrap_or(48_000) as i32;
            let status = CMAudioSampleBufferCreateReadyWithPacketDescriptions(None, &block, format, frames as isize, CMTime::new(at, rate), null(), NonNull::from(&mut sample));
            if status != 0 || sample.is_null() {
                return;
            }
            let sample = CFRetained::from_raw(NonNull::new_unchecked(sample));
            input.appendSampleBuffer(&sample);
        })
    }

    /// Ends the video at slot `end` (the last frame shown till then: the window may have been
    /// hidden at the end) and closes the file. Returns what was written.
    pub(super) fn finish(mut self, end: u64) -> Result<Summary, String> {
        autoreleasepool(|_| unsafe {
            if let Some((last, at)) = &self.last {
                for gap in at + 1..end {
                    if self.video.isReadyForMoreMediaData() && self.adaptor.appendPixelBuffer_withPresentationTime(last, CMTime::new(gap as i64, self.settings.fps as i32)) {
                        self.summary.repeated += 1;
                    }
                }
            }
            let end = self.last.as_ref().map_or(0, |(_, slot)| end.max(slot + 1));
            self.last = None;
            if self.failed() || end == 0 {
                self.writer.cancelWriting();
                return Err(self.writer.error().map_or("nothing was recorded".into(), |e| e.localizedDescription().to_string()));
            }
            self.writer.endSessionAtSourceTime(CMTime::new(end as i64, self.settings.fps as i32));
            self.video.markAsFinished();
            if let Some((input, _)) = &self.audio {
                input.markAsFinished();
            }
            // On this thread, not the game's: it waits for the encoders to finish.
            #[allow(deprecated)]
            let done = self.writer.finishWriting();
            if !done || self.writer.status() != AVAssetWriterStatus::Completed {
                return Err(self.writer.error().map_or("the file could not be finished".into(), |e| e.localizedDescription().to_string()));
            }
            self.summary.seconds = end as f64 / self.settings.fps as f64;
            Ok(self.summary)
        })
    }
}
