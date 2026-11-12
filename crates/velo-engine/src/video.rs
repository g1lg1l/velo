//! General video (H.264, HEVC, ProRes…) on macOS: AVFoundation decodes it, in hardware where
//! it can, into BGRA frames read one after another ([`Decoder`]) on a movie's reader thread
//! (media.rs). A frame due behind the last one read, or far ahead, starts a new reader there.
//! A movie shot with the phone turned says so in its track's transform and keeps its frames as
//! the sensor saw them: each is turned upright as it's copied out ([`turned`]).
//! Cameras too ([`camera`]): a capture session's frames, as they come, for a Live node.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use dispatch2::DispatchQueue;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2_core_foundation::CFRetained;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderStatus, AVAssetReaderTrackOutput, AVAssetTrack, AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMTimeRange};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// A movie AVFoundation plays: where it is, its frame rate, and how many quarter turns clockwise
/// its frames are stored short of upright ([`turns`]).
#[derive(Debug)]
pub struct Clip {
    pub path: PathBuf,
    pub fps: f64,
    turns: u8,
}

/// The quarter turns clockwise a track's preferred transform asks for, from where it sends the x
/// axis (`a`, `b`; y runs down): right is upright, down a quarter turn, left a half, up three.
// shortcut: a mirrored transform shows unmirrored (cameras don't write one); read `c` and `d` too if one turns up
fn turns(a: f64, b: f64) -> u8 {
    match (a.round() as i32, b.round() as i32) {
        (0, 1) => 1,
        (-1, 0) => 2,
        (0, -1) => 3,
        _ => 0,
    }
}

/// A `w`×`h` frame of 4-byte pixels, its rows `stride` bytes apart in `src`, into `out` with its
/// rows packed, turned `turns` quarter turns clockwise (an odd number swaps its sides).
fn turned(src: &[u8], [w, h]: [usize; 2], stride: usize, turns: u8, out: &mut Vec<u8>) {
    if turns.is_multiple_of(4) {
        out.clear();
        for row in 0..h {
            // the buffer's rows, without the padding at their ends
            out.extend_from_slice(&src[row * stride..][..w * 4]);
        }
        return;
    }
    let (ow, oh) = if turns % 2 == 1 { (h, w) } else { (w, h) };
    // every byte is written below: nothing to clear in a buffer that's the right size already
    out.resize(ow * oh * 4, 0);
    // the pixel of the frame that lands at x, y
    let from = |x: usize, y: usize| match turns % 4 {
        1 => (y, h - 1 - x),
        2 => (w - 1 - x, h - 1 - y),
        _ => (w - 1 - y, x),
    };
    // eight rows at a time: a quarter turn reads down the frame's columns, and this way the
    // eight pixels it takes from each of the frame's rows come out of the cache together
    for band in (0..oh).step_by(8) {
        for x in 0..ow {
            for y in band..(band + 8).min(oh) {
                let (sx, sy) = from(x, y);
                out[(y * ow + x) * 4..][..4].copy_from_slice(&src[sy * stride + sx * 4..][..4]);
            }
        }
    }
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
        let transform = unsafe { track.preferredTransform() };
        let turns = turns(transform.a, transform.b);
        // as it shows: a movie on its side is as wide as its frames are tall
        let size = if turns % 2 == 1 { [size.height, size.width] } else { [size.width, size.height] }.map(|side| side.round() as u32);
        if size.contains(&0) || size[0].max(size[1]) > 8192 {
            return Err(format!("a {}×{} video: too big for a texture", size[0], size[1]));
        }
        let fps = f64::from(unsafe { track.nominalFrameRate() });
        let fps = if fps > 0.0 { fps } else { 30.0 };
        let seconds = unsafe { asset.duration().seconds() };
        let frames = ((seconds * fps).floor() as usize).clamp(1, 1 << 20);
        let ends = (1..=frames).map(|i| i as f64 / fps).collect();
        Ok((Clip { path: path.to_path_buf(), fps, turns }, size, ends))
    })
}

/// The output settings that ask for BGRA frames.
fn bgra() -> Retained<NSDictionary<NSString, AnyObject>> {
    let key: &NSString = unsafe { &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString) };
    let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[format.as_ref()])
}

/// A BGRA frame's rows into `out`, packed and turned `turns` quarter turns clockwise; false if
/// it can't be read (`out` is empty for a frame of another layout).
fn rows(image: &CVPixelBuffer, turns: u8, out: &mut Vec<u8>) -> bool {
    let (w, h) = (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image));
    if unsafe { CVPixelBufferLockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) } != 0 {
        return false;
    }
    let (base, stride) = (CVPixelBufferGetBaseAddress(image) as *const u8, CVPixelBufferGetBytesPerRow(image));
    if !base.is_null() && stride >= w * 4 && h > 0 {
        // the buffer's own rows, read in place (its last has no padding to count on)
        turned(unsafe { std::slice::from_raw_parts(base, stride * (h - 1) + w * 4) }, [w, h], stride, turns, out);
    } else {
        out.clear();
    }
    unsafe { CVPixelBufferUnlockBaseAddress(image, CVPixelBufferLockFlags::ReadOnly) };
    true
}

/// Frames in order from a point in a movie, as BGRA rows, upright.
pub struct Decoder {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
    turns: u8,
}

impl Decoder {
    /// Reading from `start` seconds on.
    pub fn new(clip: &Clip, start: f64) -> Result<Self, String> {
        autoreleasepool(|_| {
            let asset = asset(&clip.path);
            let track = track(&asset).ok_or("no video track")?;
            let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }.map_err(|e| e.localizedDescription().to_string())?;
            // BGRA, as the texture takes it; the system converts while it decodes
            let output = unsafe { AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(&track, Some(&bgra())) };
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
            Ok(Self { reader, output, turns: clip.turns })
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
                if !rows(&image, self.turns, out) {
                    continue;
                }
                return Some(time);
            }
        })
    }

    /// Read to the end, or failed.
    pub fn done(&self) -> bool {
        (unsafe { self.reader.status() }) != AVAssetReaderStatus::Reading
    }
}

// ---------------------------------------------------------------------------------------------
// Cameras

/// A camera's newest frame, held (not copied) from the capture queue to its reader thread.
struct Held(CFRetained<CVPixelBuffer>);
// SAFETY: a pixel buffer may be read from any thread; it's only locked and read, by one at a time
unsafe impl Send for Held {}

type Slot = Arc<(Mutex<Option<Held>>, Condvar)>;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Grabber has no Drop
    #[unsafe(super(NSObject))]
    #[name = "VeloCameraGrabber"]
    #[ivars = Slot]
    struct Grabber;

    unsafe impl NSObjectProtocol for Grabber {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for Grabber {
        /// On the capture queue: the frame kept, the one before it not yet taken let go.
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn frame(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _connection: &AVCaptureConnection) {
            let Some(image) = (unsafe { sample.image_buffer() }) else { return };
            let (newest, came) = &**self.ivars();
            if let Ok(mut newest) = newest.lock() {
                *newest = Some(Held(image));
                came.notify_one();
            }
        }
    }
);

#[allow(deprecated)] // the plain device list: built-in, USB and capture cards, without naming each kind
fn devices() -> Vec<(String, Retained<AVCaptureDevice>)> {
    let Some(video) = (unsafe { AVMediaTypeVideo }) else { return Vec::new() };
    unsafe { AVCaptureDevice::devicesWithMediaType(video) }.iter().map(|d| (unsafe { d.localizedName() }.to_string(), d)).collect()
}

/// Whether macOS is still to say if Velo may use the camera: it asks the first time one opens,
/// and a camera's first frame waits for the answer.
pub fn asking() -> bool {
    unsafe { AVMediaTypeVideo }.is_some_and(|video| unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) } == AVAuthorizationStatus::NotDetermined)
}

/// The cameras macOS has, by name.
pub fn cameras() -> Vec<String> {
    autoreleasepool(|_| devices().into_iter().map(|d| d.0).collect())
}

/// The camera called `name` (or whose name holds it), read until `stop` says so, each frame's
/// size, BGRA rows and bytes a row to `each`. Err when there's none by that name, or macOS
/// won't open it. The first time, macOS asks whether Velo may use the camera: this waits for
/// the answer ([`asking`] says so meanwhile).
pub fn camera(name: &str, stop: impl Fn() -> bool, mut each: impl FnMut([u32; 2], &[u8], usize)) -> Result<(), String> {
    let video = unsafe { AVMediaTypeVideo }.ok_or("no video on this Mac")?;
    const REFUSED: &str = "macOS doesn't let Velo use the camera: allow it in System Settings, Privacy & Security, Camera";
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) };
    if status == AVAuthorizationStatus::Denied || status == AVAuthorizationStatus::Restricted {
        return Err(REFUSED.into());
    }
    if status == AVAuthorizationStatus::NotDetermined {
        let (tx, answer) = std::sync::mpsc::channel();
        let answered = block2::RcBlock::new(move |yes: Bool| {
            let _ = tx.send(yes.as_bool());
        });
        unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &answered) };
        loop {
            match answer.recv_timeout(Duration::from_millis(100)) {
                Ok(true) => break,
                Ok(false) => return Err(REFUSED.into()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !stop() => {}
                Err(_) => return Ok(()),
            }
        }
    }
    let all = devices();
    let (_, device) = all.iter().find(|d| d.0 == name).or_else(|| all.iter().find(|d| d.0.contains(name))).ok_or_else(|| format!("no camera called {name:?}"))?;
    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(device) }.map_err(|e| format!("{name}: {}", e.localizedDescription()))?;
    let output = unsafe { AVCaptureVideoDataOutput::new() };
    unsafe { output.setVideoSettings(Some(&bgra())) };
    // a frame the reader hasn't taken is replaced, never queued
    unsafe { output.setAlwaysDiscardsLateVideoFrames(true) };
    let slot = Slot::default();
    let grabber: Retained<Grabber> = unsafe { msg_send![super(Grabber::alloc().set_ivars(slot.clone())), init] };
    let queue = DispatchQueue::new("velo camera", None);
    unsafe { output.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(&*grabber)), Some(&queue)) };
    let session = unsafe { AVCaptureSession::new() };
    if !unsafe { session.canAddInput(&input) && session.canAddOutput(&output) } {
        return Err(format!("{name}: the camera won't open"));
    }
    unsafe {
        session.addInput(&input);
        session.addOutput(&output);
        session.startRunning();
    }
    while !stop() {
        let (newest, came) = &*slot;
        let Ok(waiting) = newest.lock() else { break };
        // woken now and then to see whether to stop
        let Ok((mut newest, _)) = came.wait_timeout_while(waiting, Duration::from_millis(100), |n| n.is_none()) else { break };
        let Some(Held(image)) = newest.take() else { continue };
        drop(newest);
        if unsafe { CVPixelBufferLockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) } != 0 {
            continue;
        }
        let (w, h) = (CVPixelBufferGetWidth(&image), CVPixelBufferGetHeight(&image));
        let (base, stride) = (CVPixelBufferGetBaseAddress(&image) as *const u8, CVPixelBufferGetBytesPerRow(&image));
        if !base.is_null() && stride >= w * 4 && h > 0 {
            // the buffer's own rows, read in place: the one copy is the reader's
            each([w as u32, h as u32], unsafe { std::slice::from_raw_parts(base, stride * (h - 1) + w * 4) }, stride);
        }
        unsafe { CVPixelBufferUnlockBaseAddress(&image, CVPixelBufferLockFlags::ReadOnly) };
    }
    unsafe {
        session.stopRunning();
        output.setSampleBufferDelegate_queue(None, None);
    }
    Ok(())
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
    fn a_tracks_transform_says_how_far_its_frames_are_turned() {
        // as phones write them: upright, held upright (ffprobe's rotation -90), upside down, the other way up
        assert_eq!([turns(1.0, 0.0), turns(0.0, 1.0), turns(-1.0, 0.0), turns(0.0, -1.0)], [0, 1, 2, 3]);
        assert_eq!(turns(0.7, 0.7), 0, "anything else plays as it is");
    }

    #[test]
    fn a_frame_turns_by_quarters() {
        // three pixels over three, numbered along its rows, each row with a pixel of padding
        //   0 1 2
        //   3 4 5
        let src: Vec<u8> = [0, 1, 2, 99, 3, 4, 5].iter().flat_map(|&p| [p; 4]).collect();
        let turn = |turns| {
            let mut out = vec![7; 3];
            turned(&src, [3, 2], 16, turns, &mut out);
            out.chunks(4).map(|p| p[0]).collect::<Vec<_>>()
        };
        assert_eq!(turn(0), [0, 1, 2, 3, 4, 5], "as it is, without the padding");
        assert_eq!(turn(1), [3, 0, 4, 1, 5, 2], "clockwise: its left side on top");
        assert_eq!(turn(2), [5, 4, 3, 2, 1, 0], "upside down");
        assert_eq!(turn(3), [2, 5, 1, 4, 0, 3], "counterclockwise: its right side on top");
    }

    /// The fixture again, its track saying a quarter turn clockwise (`ffmpeg -display_rotation
    /// -90 -i h264.mp4 -c copy`), as a phone held upright writes its movies.
    #[test]
    fn a_movie_shot_on_its_side_plays_upright() {
        let (clip, size, _) = open(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/h264-turned.mp4")).unwrap();
        assert_eq!(size, [48, 96], "as tall as its frames are wide");
        let mut frame = Vec::new();
        Decoder::new(&clip, 0.0).unwrap().next(&mut frame).unwrap();
        assert_eq!(frame.len(), 48 * 96 * 4);
        // red | green over blue, a quarter turn on: blue on the left, red over green on the right
        assert_eq!([lit(&frame, 48, 35, 24), lit(&frame, 48, 35, 72), lit(&frame, 48, 11, 48)], [Some(2), Some(1), Some(0)]);
    }

    #[test]
    fn what_isnt_a_movie_says_so() {
        assert!(open(Path::new("/nowhere/at/all.mp4")).is_err());
    }

    /// Needs a camera, and macOS' leave to use it: `cargo test -p velo-engine a_camera -- --ignored`.
    #[test]
    #[ignore]
    fn a_camera_sends_frames() {
        let name = cameras().into_iter().next().expect("a camera");
        let (started, got) = (std::time::Instant::now(), std::cell::Cell::new(None));
        camera(&name, || got.get().is_some() || started.elapsed().as_secs() > 10, |size, pixels, stride| got.set(Some((size, pixels.len(), stride)))).expect("opens");
        let (size, bytes, stride) = got.get().expect("a frame in 10 s");
        assert!(size[0] > 0 && stride == size[0] as usize * 4 && bytes == stride * size[1] as usize, "{name}: {size:?}, {bytes} bytes");
        eprintln!("{name}: {size:?}");
    }
}
