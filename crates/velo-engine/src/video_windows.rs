//! General video (H.264, HEVC…) on Windows: Media Foundation's source reader decodes it and
//! converts it to BGRA, frames read one after another ([`Decoder`]) on a movie's reader
//! thread (media.rs), as video.rs does on macOS. A frame due behind the last one read, or far
//! ahead, starts a new reader there.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{Error, GUID, HSTRING, Interface, PWSTR};

/// A movie Media Foundation plays: where it is, its frame rate, its picture's size.
#[derive(Debug)]
pub struct Clip {
    pub path: PathBuf,
    pub fps: f64,
    pub size: [u32; 2],
    /// Its first frame's time in the file: the MP4 reader skips edit lists, so a movie with
    /// B frames starts a frame or two late.
    offset: f64,
}

/// The first video stream (the reader numbers its special streams below zero).
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Media Foundation, started once, and COM on the thread asking: each thread that reads.
fn start() -> Result<(), String> {
    thread_local! {
        // a thread with COM up already (an STA, say) keeps it
        static COM: () = { let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }; };
    }
    COM.with(|_| {});
    static UP: OnceLock<Result<(), String>> = OnceLock::new();
    UP.get_or_init(|| unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.map_err(|e| e.message())).clone()
}

/// What went wrong, in words for the node.
fn why(e: Error) -> String {
    match e.code() {
        MF_E_UNSUPPORTED_BYTESTREAM_TYPE | MF_E_INVALIDSTREAMNUMBER => "no video in this file (or a format Windows doesn't play)".into(),
        MF_E_TOPO_CODEC_NOT_FOUND => "no decoder for this video here (HEVC wants the HEVC Video Extensions)".into(),
        _ => e.message(),
    }
}

/// A reader of a movie's first video stream, its frames as RGB32 (BGRA in memory).
fn reader(path: &Path) -> windows::core::Result<IMFSourceReader> {
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.ok_or_else(Error::empty)?;
        // converted by the video processor (2.5 times the plain one's speed at 1080p);
        // ponytail: decoded on the CPU, hardware wants a D3D11 device handed over
        // (MF_SOURCE_READER_D3D_MANAGER) and its frames copied back, if 4K stutters
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
        let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), &attributes)?;
        // the picture only: no sound decoded
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(VIDEO, true)?;
        let rgb = MFCreateMediaType()?;
        rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(VIDEO, None, &rgb)?;
        Ok(reader)
    }
}

/// Two numbers packed in a media type's attribute: width and height, or a frame rate's
/// numerator and denominator.
fn pair(v: u64) -> [u32; 2] {
    [(v >> 32) as u32, v as u32]
}

/// A movie's video: its size, and when each frame ends (seconds, at its nominal frame rate).
pub fn open(path: &Path) -> Result<(Clip, [u32; 2], Vec<f64>), String> {
    start()?;
    let reader = reader(path).map_err(why)?;
    let (size, rate, duration) = unsafe {
        // the picture's size as the file has it (a decoder may pad its frames)
        let native = reader.GetNativeMediaType(VIDEO, 0).map_err(why)?;
        let size = pair(native.GetUINT64(&MF_MT_FRAME_SIZE).map_err(why)?);
        let rate = native.GetUINT64(&MF_MT_FRAME_RATE).map(pair).unwrap_or_default();
        let duration = reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION).map_err(why)?;
        (size, rate, duration.Anonymous.Anonymous.Anonymous.uhVal)
    };
    if size.contains(&0) || size[0].max(size[1]) > 8192 {
        return Err(format!("a {}×{} video: too big for a texture", size[0], size[1]));
    }
    let fps = f64::from(rate[0]) / f64::from(rate[1].max(1));
    let fps = if fps > 0.0 { fps } else { 30.0 };
    // in units of 100 ns
    let seconds = duration as f64 / 1e7;
    let frames = ((seconds * fps).floor() as usize).clamp(1, 1 << 20);
    let ends = (1..=frames).map(|i| i as f64 / fps).collect();
    // the first frame's time, read from it: one that won't decode says so now
    let frame = frame(&reader).map_err(why)?;
    let mut first = Decoder { reader, size, frame, from: f64::MIN, offset: 0.0, ended: false };
    let offset = first.next(&mut Vec::new()).ok_or("no frame of it decodes here")?;
    Ok((Clip { path: path.to_path_buf(), fps, size, offset }, size, ends))
}

/// Frames in order from a point in a movie, as BGRA rows.
pub struct Decoder {
    reader: IMFSourceReader,
    /// The picture's size, and the frames' as they come (the decoder's, maybe padded).
    size: [u32; 2],
    frame: [u32; 2],
    /// Frames before this (seconds) are skipped: a seek lands on the key frame before.
    from: f64,
    offset: f64,
    ended: bool,
}

impl Decoder {
    /// Reading from `start` seconds on.
    pub fn new(clip: &Clip, start: f64) -> Result<Self, String> {
        self::start()?;
        let reader = reader(&clip.path).map_err(why)?;
        if start > 0.0 {
            let mut at = PROPVARIANT::default();
            unsafe {
                let v = &mut *at.Anonymous.Anonymous;
                (v.vt, v.Anonymous.hVal) = (VT_I8, ((start + clip.offset) * 1e7) as i64);
                // a null GUID: the position in 100 ns units
                reader.SetCurrentPosition(&GUID::zeroed(), &at).map_err(why)?;
            }
        }
        let frame = frame(&reader).map_err(why)?;
        Ok(Self { reader, size: clip.size, frame, from: start - 0.5 / clip.fps, offset: clip.offset, ended: false })
    }

    /// The next frame into `out` (BGRA, rows packed), with its time in seconds; none at the end.
    pub fn next(&mut self, out: &mut Vec<u8>) -> Option<f64> {
        while !self.ended {
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            let read = unsafe { self.reader.ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample)) };
            if read.is_err() || flags & (MF_SOURCE_READERF_ENDOFSTREAM.0 | MF_SOURCE_READERF_ERROR.0) as u32 != 0 {
                self.ended = true;
            }
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 {
                self.frame = frame(&self.reader).ok()?;
            }
            // a gap in the stream carries no picture
            let Some(sample) = sample else { continue };
            let time = time as f64 / 1e7 - self.offset;
            if time < self.from {
                continue;
            }
            self.copy(&sample, out);
            return Some(time);
        }
        None
    }

    /// A frame's picture into `out`: its rows, without the padding at their ends or below.
    fn copy(&self, sample: &IMFSample, out: &mut Vec<u8>) {
        out.clear();
        let Ok(buffer) = (unsafe { sample.ConvertToContiguousBuffer() }) else { return };
        let (mut top, mut pitch, mut start, mut length) = (std::ptr::null_mut(), 0i32, std::ptr::null_mut(), 0u32);
        // a 2D buffer says where its top row is and how far the next one is (up, if negative)
        if let Ok(b2) = buffer.cast::<IMF2DBuffer2>() {
            if unsafe { b2.Lock2DSize(MF2DBuffer_LockFlags_Read, &mut top, &mut pitch, &mut start, &mut length) }.is_ok() {
                let bytes = unsafe { std::slice::from_raw_parts(start, length as usize) };
                rows(out, bytes, top as isize - start as isize, pitch as isize, self.size);
                let _ = unsafe { b2.Unlock2D() };
            }
        } else if unsafe { buffer.Lock(&mut start, None, Some(&mut length)) }.is_ok() {
            let bytes = unsafe { std::slice::from_raw_parts(start, length as usize) };
            rows(out, bytes, 0, self.frame[0] as isize * 4, self.size);
            let _ = unsafe { buffer.Unlock() };
        }
    }

    /// Read to the end, or failed.
    pub fn done(&self) -> bool {
        self.ended
    }
}

/// The cameras Windows has, each with its name, as Media Foundation lists them.
fn devices() -> Result<Vec<(String, IMFActivate)>, String> {
    start()?;
    // SAFETY: Media Foundation's own calls; the array it allocates holds activation objects we
    // take (each slot left empty) and is freed once, as are the names it allocates
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(why)?;
        let attributes = attributes.ok_or("Media Foundation: no attributes")?;
        attributes.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID).map_err(why)?;
        let (mut all, mut count) = (std::ptr::null_mut(), 0u32);
        MFEnumDeviceSources(&attributes, &mut all, &mut count).map_err(why)?;
        let mut found = Vec::new();
        for i in 0..count as usize {
            let Some(activate) = (*all.add(i)).take() else { continue };
            let (mut text, mut len) = (PWSTR::null(), 0u32);
            let name = match activate.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut text, &mut len) {
                Ok(()) => {
                    let name = text.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(text.0.cast()));
                    name
                }
                Err(_) => String::new(),
            };
            found.push((name, activate));
        }
        CoTaskMemFree(Some(all.cast()));
        Ok(found)
    }
}

/// The cameras Windows has, by name.
pub fn cameras() -> Vec<String> {
    devices().map(|d| d.into_iter().map(|c| c.0).collect()).unwrap_or_default()
}

/// The camera called `name` (or whose name holds it), read until `stop` says so, each frame's
/// size, BGRA rows and bytes a row to `each`. Err when there's none by that name, or Windows
/// won't open it (its camera privacy settings may say no to desktop apps).
pub fn camera(name: &str, stop: impl Fn() -> bool, mut each: impl FnMut([u32; 2], &[u8], usize)) -> Result<(), String> {
    let all = devices()?;
    let (_, activate) = all.iter().find(|d| d.0 == name).or_else(|| all.iter().find(|d| d.0.contains(name))).ok_or_else(|| format!("no camera called {name:?}"))?;
    // SAFETY: Media Foundation's own calls on objects made here; the camera is shut down after
    unsafe {
        let source: IMFMediaSource = activate.ActivateObject().map_err(|e| format!("{name}: {} (Windows' camera privacy settings may say no)", why(e)))?;
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(why)?;
        let attributes = attributes.ok_or("Media Foundation: no attributes")?;
        // what the camera sends (YUY2, NV12, MJPEG) converted to RGB32 by the video processor
        attributes.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1).map_err(why)?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attributes).map_err(why)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false).map_err(why)?;
        reader.SetStreamSelection(VIDEO, true).map_err(why)?;
        let rgb = MFCreateMediaType().map_err(why)?;
        rgb.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(why)?;
        rgb.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).map_err(why)?;
        reader.SetCurrentMediaType(VIDEO, None, &rgb).map_err(why)?;
        let size = frame(&reader).map_err(why)?;
        let mut decoder = Decoder { reader, size, frame: size, from: f64::MIN, offset: 0.0, ended: false };
        let mut out = Vec::new();
        // a camera has no end: a frame that doesn't come is the camera gone
        while !stop() && decoder.next(&mut out).is_some() {
            if !out.is_empty() {
                each(size, &out, size[0] as usize * 4);
            }
        }
        let _ = source.Shutdown();
    }
    Ok(())
}

/// The size of the frames the reader hands out now.
fn frame(reader: &IMFSourceReader) -> windows::core::Result<[u32; 2]> {
    unsafe { Ok(pair(reader.GetCurrentMediaType(VIDEO)?.GetUINT64(&MF_MT_FRAME_SIZE)?)) }
}

/// `size` of a frame's rows, the first at `top` in `bytes` and the next `pitch` on, into
/// `out`; nothing if any falls outside.
fn rows(out: &mut Vec<u8>, bytes: &[u8], top: isize, pitch: isize, [w, h]: [u32; 2]) {
    let line = w as usize * 4;
    for row in 0..h as isize {
        let at = top + row * pitch;
        let Some(row) = usize::try_from(at).ok().and_then(|at| bytes.get(at..at + line)) else {
            out.clear();
            return;
        };
        out.extend_from_slice(row);
    }
    // RGB32 leaves its fourth byte unset: opaque
    out.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
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
        // BGRA: red is 2, green 1, blue 0; opaque
        assert_eq!([lit(&frame, 96, 12, 6), lit(&frame, 96, 84, 6), lit(&frame, 96, 48, 39)], [Some(2), Some(1), Some(0)]);
        assert!(frame.iter().skip(3).step_by(4).all(|&a| a == 255));
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
    fn rows_drop_padding_and_follow_the_pitch() {
        // 2×2 pixels in a buffer 3 pixels wide, stored bottom up
        let bytes: Vec<u8> = (0..24).collect();
        let mut out = Vec::new();
        rows(&mut out, &bytes, 12, -12, [2, 2]);
        assert_eq!(out, [12, 13, 14, 255, 16, 17, 18, 255, 0, 1, 2, 255, 4, 5, 6, 255]);
        rows(&mut out, &bytes, 12, 12, [2, 2]);
        assert!(out.is_empty(), "a row past the end: nothing");
    }

    #[test]
    fn cameras_are_listed_without_opening_any() {
        // whatever this computer has, by name; none is fine
        let names = cameras();
        assert!(names.iter().all(|n| !n.is_empty()), "{names:?}");
        assert!(camera("no such camera", || true, |_, _, _| {}).is_err());
    }

    /// The first camera for a second: frames of its size come. Lights the camera up, so run by
    /// hand (`--ignored`) where one is allowed.
    #[test]
    #[ignore]
    fn a_camera_gives_frames() {
        let Some(first) = cameras().into_iter().next() else {
            return eprintln!("no camera here: skipped");
        };
        let (seen, start) = (std::cell::Cell::new(0), std::time::Instant::now());
        camera(&first, || seen.get() >= 10 || start.elapsed().as_secs() > 5, |size, bytes, stride| {
            assert_eq!(bytes.len(), stride * size[1] as usize);
            seen.set(seen.get() + 1);
        })
        .unwrap();
        assert!(seen.get() > 0, "{first}: no frames in 5 s");
    }

    #[test]
    fn what_isnt_a_movie_says_so() {
        assert!(open(Path::new("C:/nowhere/at/all.mp4")).is_err());
    }
}
