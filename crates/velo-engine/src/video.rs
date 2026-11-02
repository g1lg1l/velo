//! General video (H.264, HEVC, ProRes…) on macOS: AVFoundation decodes it, in hardware where
//! it can, into BGRA frames read one after another ([`Decoder`]) on a movie's reader thread
//! (media.rs). A frame due behind the last one read, or far ahead, starts a new reader there.

use std::path::{Path, PathBuf};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVMediaTypeVideo, AVURLAsset};
use objc2_core_media::{CMTime, CMTimeRange};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// A movie AVFoundation plays: where it is, its frame rate.
#[derive(Debug)]
pub struct Clip {
    pub path: PathBuf,
    pub fps: f64,
}

fn asset(path: &Path) -> Retained<AVURLAsset> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) }
}

#[allow(deprecated)] // the synchronous track list: this runs on a loading thread anyway
fn track(asset: &AVURLAsset) -> Option<Retained<AVAssetTrack>> {
    let video = unsafe { AVMediaTypeVideo }?;
    unsafe { asset.tracksWithMediaType(video) }.firstObject()
}

/// A movie's video: its size, and when each frame ends (seconds, at its nominal frame rate).
pub fn open(path: &Path) -> Result<(Clip, [u32; 2], Vec<f64>), String> {
    autoreleasepool(|_| {
        let asset = asset(path);
        let track = track(&asset).ok_or("no video in this file (or a format macOS doesn't play)")?;
        let size = unsafe { track.naturalSize() };
        let size = [size.width.round() as u32, size.height.round() as u32];
        if size.contains(&0) || size[0].max(size[1]) > 8192 {
            return Err(format!("a {}×{} video: too big for a texture", size[0], size[1]));
        }
        let fps = f64::from(unsafe { track.nominalFrameRate() });
        let fps = if fps > 0.0 { fps } else { 30.0 };
        let seconds = unsafe { asset.duration().seconds() };
        let frames = ((seconds * fps).floor() as usize).clamp(1, 1 << 20);
        let ends = (1..=frames).map(|i| i as f64 / fps).collect();
        Ok((Clip { path: path.to_path_buf(), fps }, size, ends))
    })
}

/// Frames in order from a point in a movie, as BGRA rows.
pub struct Decoder {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
}

impl Decoder {
    /// Reading from `start` seconds on.
    pub fn new(clip: &Clip, start: f64) -> Result<Self, String> {
        autoreleasepool(|_| {
            let asset = asset(&clip.path);
            let track = track(&asset).ok_or("no video track")?;
            let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }.map_err(|e| e.localizedDescription().to_string())?;
            // BGRA, as the texture takes it; the system converts while it decodes
            let key: &NSString = unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString) };
            let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
            let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[format.as_ref()]);
            let output = unsafe { AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&settings)) };
            // the frames are only read: no copy of each one
            unsafe { output.setAlwaysCopiesSampleData(false) };
            unsafe { reader.addOutput(&output) };
            if start > 0.0 {
                let range = CMTimeRange { start: unsafe { CMTime::with_seconds(start, 600) }, duration: unsafe { CMTime::with_seconds(f64::from(u32::MAX), 600) } };
                unsafe { reader.setTimeRange(range) };
            }
            if !unsafe { reader.startReading() } {
                return Err(unsafe { reader.error() }.map_or_else(|| "the movie won't read".to_string(), |e| e.localizedDescription().to_string()));
            }
            Ok(Self { reader, output })
        })
    }

    /// The next frame into `out` (BGRA, rows packed), with its time in seconds; none at the end.
    pub fn next(&mut self, out: &mut Vec<u8>) -> Option<f64> {
        autoreleasepool(|_| {
            loop {
                let sample = unsafe { self.output.copyNextSampleBuffer() }?;
                // marker-only buffers carry no picture
                let Some(image) = (unsafe { sample.image_buffer() }) else { continue };
                let time = unsafe { sample.presentation_time_stamp().seconds() };
                let (w, h) = (CVPixelBufferGetWidth(&image), CVPixelBufferGetHeight(&image));
                if unsafe { CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) } != 0 {
                    continue;
                }
                let (base, stride) = (CVPixelBufferGetBaseAddress(&image) as *const u8, CVPixelBufferGetBytesPerRow(&image));
                out.clear();
                if !base.is_null() && stride >= w * 4 {
                    for row in 0..h {
                        // the buffer's rows, without the padding at their ends
                        let line = unsafe { std::slice::from_raw_parts(base.add(row * stride), w * 4) };
                        out.extend_from_slice(line);
                    }
                }
                unsafe { CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) };
                return Some(time);
            }
        })
    }

    /// Read to the end, or failed.
    pub fn done(&self) -> bool {
        (unsafe { self.reader.status() }) != AVAssetReaderStatus::Reading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The HAP fixtures' four frames as H.264 (ffmpeg), three times as big (Windows' decoder
    /// wants 48 px a side at least): red | green over blue, then green, blue, white, at 10 fps.
    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/h264.mp4")
    }

    /// The strongest of blue, green, red at a pixel, if it clearly stands out.
    fn lit(bgra: &[u8], w: usize, x: usize, y: usize) -> Option<usize> {
        let p = &bgra[(y * w + x) * 4..][..3];
        let max = (0..3).max_by_key(|&c| p[c])?;
        (p[max] > 180 && (0..3).filter(|&c| c != max).all(|c| p[c] < 80)).then_some(max)
    }

    #[test]
    fn a_movie_opens_and_decodes_in_order() {
        let (clip, size, ends) = open(&fixture()).unwrap();
        assert_eq!((size, ends.len()), ([96, 48], 4));
        assert!((clip.fps - 10.0).abs() < 1e-6 && (ends[3] - 0.4).abs() < 1e-6, "{} {ends:?}", clip.fps);
        let mut d = Decoder::new(&clip, 0.0).unwrap();
        let mut frame = Vec::new();
        let t = d.next(&mut frame).unwrap();
        assert!(t.abs() < 1e-6 && frame.len() == 96 * 48 * 4, "{t} {}", frame.len());
        // BGRA: red is 2, green 1, blue 0
        assert_eq!([lit(&frame, 96, 12, 6), lit(&frame, 96, 84, 6), lit(&frame, 96, 48, 39)], [Some(2), Some(1), Some(0)]);
        let times: Vec<f64> = std::iter::from_fn(|| d.next(&mut frame)).collect();
        assert_eq!(times.len(), 3, "{times:?}");
        assert_eq!(lit(&frame, 96, 48, 24), None, "the last frame is white");
        assert!(d.done());
        // from the middle: the third frame first
        let mut d = Decoder::new(&clip, 0.2).unwrap();
        assert!((d.next(&mut frame).unwrap() - 0.2).abs() < 1e-3);
        assert_eq!(lit(&frame, 96, 12, 6), Some(0), "blue");
    }

    #[test]
    fn what_isnt_a_movie_says_so() {
        assert!(open(Path::new("/nowhere/at/all.mp4")).is_err());
    }
}
