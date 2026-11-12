//! Pictures from files for Image and Video nodes (docs/rendering.md §4): a still (upright, as
//! its EXIF orientation says; HEIC and HEIF too where the system decodes them: macOS), an animated
//! GIF or a folder of numbered frames (an image sequence), decoded on a thread of their own into
//! RGBA frames in memory; or a movie (HAP, or one the system plays: H.264, HEVC, on macOS ProRes too), whose
//! frames stay in the file and are read as they come due. A graph built while a file loads shows black there and is built again once it's in;
//! the render thread uploads each frame as it comes due ([`Playing`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};

use crate::hap::{Format, Movie};

/// What an Image or Video node takes: files by extension. HEIC and HEIF (a phone's photos) only
/// where the system decodes them: macOS ([`system_still`]).
#[cfg(target_os = "macos")]
pub const EXTENSIONS: [&str; 12] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tga", "heic", "heif", "mov", "mp4", "m4v"];
#[cfg(not(target_os = "macos"))]
pub const EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tga", "mov", "mp4", "m4v"];
/// Movies: HAP in a `.mov`, or (macOS, Windows) anything the system plays.
pub const MOVIES: [&str; 3] = ["mov", "mp4", "m4v"];

/// A movie the system decodes (macOS: AVFoundation, Windows: Media Foundation).
#[cfg(any(target_os = "macos", windows))]
pub use crate::video::Clip;
/// No system decoder here yet: only HAP movies play.
#[cfg(not(any(target_os = "macos", windows)))]
#[derive(Debug)]
pub struct Clip;
/// What a folder of frames holds.
const PICTURES: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tga"];
/// A sequence's frame rate: its files carry none.
pub const SEQUENCE_FPS: f64 = 30.0;
/// ponytail: every frame decoded in memory, at most this much (about 8 s of 1080p at 30 fps);
/// longer clips want frames streamed from disk, or HAP
pub const BUDGET: usize = 2 << 30;
/// The largest side a texture takes everywhere wgpu runs (its default limit).
const MAX_SIDE: u32 = 8192;

/// A file's frames: decoded, or a movie's, where they are in it.
pub struct Media {
    pub size: [u32; 2],
    /// RGBA, 8 bits, display values as every picture in a graph holds them; rows top first.
    /// None for a movie.
    pub frames: Vec<Vec<u8>>,
    /// When each frame ends, in seconds from the loop's start: the last is the loop's length.
    pub ends: Vec<f64>,
    /// A HAP movie: its frames stay in the file.
    pub movie: Option<Movie>,
    /// Any other movie, which the system decodes; its frames stay in the file too.
    pub video: Option<Clip>,
    /// A live source ([`crate::input`]): `frames` holds its first frame, BGRA; the rest come as
    /// they're sent.
    pub live: Option<Arc<crate::input::Feed>>,
    /// A 3D model ([`crate::model`]), for a Particles node to be born on or a Mesh node to draw:
    /// no frames.
    pub model: Option<crate::model::Model>,
}

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} frames of {}×{}", self.count(), self.size[0], self.size[1])
    }
}

impl Media {
    /// A folder is a sequence (its images by name, numbers in order: "f2" before "f10"), a
    /// GIF plays its frames at their delays, anything else is one still.
    pub fn load(path: &Path) -> Result<Self, String> {
        if path.is_dir() {
            return sequence(path);
        }
        if crate::model::is_model(path) {
            return crate::model::load(path).map(|model| Self { model: Some(model), ..Self::empty() });
        }
        let name = || path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if MOVIES.contains(&ext.as_str()) {
            // HAP first (its frames go to the GPU as they are), else the system's decoder
            if ext == "mov" {
                match crate::hap::open(path) {
                    Ok((movie, size, ends)) => return Ok(Self { size, frames: Vec::new(), ends, movie: Some(movie), video: None, live: None, model: None }),
                    Err(e) if !e.starts_with("not HAP") => return Err(format!("{}: {e}", name())),
                    Err(_) => {}
                }
            }
            #[cfg(any(target_os = "macos", windows))]
            {
                let (clip, size, ends) = crate::video::open(path).map_err(|e| format!("{}: {e}", name()))?;
                return Ok(Self { size, frames: Vec::new(), ends, movie: None, video: Some(clip), live: None, model: None });
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            return Err(format!("{}: only HAP movies play here for now (ffmpeg -i in.mov -c:v hap -format hap_q out.mov)", name()));
        }
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gif")) {
            use image::AnimationDecoder;
            let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", name()))?;
            let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file)).map_err(|e| format!("{}: {e}", name()))?;
            let (mut media, mut end) = (Self::empty(), 0.0);
            for frame in decoder.into_frames() {
                let frame = frame.map_err(|e| format!("{}: {e}", name()))?;
                let (n, d) = frame.delay().numer_denom_ms();
                // as browsers do: a delay under 20 ms (often 0) plays at 100 ms
                let ms = f64::from(n) / f64::from(d.max(1));
                end += if ms < 20.0 { 0.1 } else { ms / 1000.0 };
                media.push(frame.into_buffer(), end).map_err(|e| format!("{}: {e}", name()))?;
            }
            return if media.frames.is_empty() { Err(format!("{}: no frames", name())) } else { Ok(media) };
        }
        let picture = still(path).map_err(|e| format!("{}: {e}", name()))?;
        let mut media = Self::empty();
        media.push(picture, 1.0).map_err(|e| format!("{}: {e}", name()))?;
        Ok(media)
    }

    /// One still, made rather than read (a Text node's words).
    pub fn still(picture: image::RgbaImage) -> Result<Self, String> {
        let mut media = Self::empty();
        media.push(picture, 1.0)?;
        Ok(media)
    }

    fn empty() -> Self {
        Self { size: [0, 0], frames: Vec::new(), ends: Vec::new(), movie: None, video: None, live: None, model: None }
    }

    /// How many frames it has.
    pub fn count(&self) -> usize {
        self.ends.len()
    }

    /// A Hap Q movie: its texels hold scaled YCoCg, the Image shader turns them to RGB.
    pub fn ycocg(&self) -> bool {
        self.movie.as_ref().is_some_and(|m| m.format == Format::YCoCg)
    }

    /// A frame more, ending at `end`: as big as the first, within the budget.
    fn push(&mut self, picture: image::RgbaImage, end: f64) -> Result<(), String> {
        let size = [picture.width(), picture.height()];
        if self.frames.is_empty() {
            if size[0].max(size[1]) > MAX_SIDE || size.contains(&0) {
                return Err(format!("{}×{} is too big for a texture (at most {MAX_SIDE} a side)", size[0], size[1]));
            }
            self.size = size;
        } else if size != self.size {
            return Err(format!("a frame is {}×{}, the first {}×{}", size[0], size[1], self.size[0], self.size[1]));
        }
        if (self.frames.len() + 1) * picture.len() > BUDGET {
            return Err(format!("more than {} frames at {}×{}: too long to hold in memory (shorter, or smaller)", self.frames.len(), size[0], size[1]));
        }
        self.frames.push(picture.into_raw());
        self.ends.push(end);
        Ok(())
    }

    /// The frame due `secs` into playback, looping.
    pub fn frame_at(&self, secs: f64) -> usize {
        self.frame_in(secs, 0.0, 1.0)
    }

    /// The frame due `secs` into playback of the part from `from` to `to` (fractions of the
    /// clip, either way round), looping that part; a part shorter than a frame holds it.
    pub fn frame_in(&self, secs: f64, from: f64, to: f64) -> usize {
        let length = self.length();
        if length <= 0.0 {
            return 0;
        }
        let (a, b) = (from.min(to).clamp(0.0, 1.0) * length, from.max(to).clamp(0.0, 1.0) * length);
        let t = if b - a > 1e-6 { a + secs.rem_euclid(b - a) } else { a };
        self.ends.partition_point(|&e| e <= t).min(self.count() - 1)
    }

    /// How long the loop plays at speed 1, in seconds (a still: 0).
    pub fn length(&self) -> f64 {
        if self.count() < 2 { 0.0 } else { self.ends.last().copied().unwrap_or(0.0) }
    }
}

/// Frames picked out of a clip, for an agent to look at.
pub struct Sampled {
    pub size: [u32; 2],
    /// Seconds (a still: 0).
    pub length: f64,
    /// When each frame starts, and its picture (RGBA, rows top first).
    pub frames: Vec<(f64, image::RgbaImage)>,
}

/// The frames of a movie, GIF, image sequence or still at `times` (seconds, held inside the clip),
/// or, with none given, `count` spread evenly over it.
pub fn sample(path: &Path, times: &[f64], count: usize) -> Result<Sampled, String> {
    let media = Media::load(path)?;
    if media.model.is_some() {
        return Err("a 3D model has no frames to look at".into());
    }
    let length = media.length();
    if media.movie.is_some() {
        // shortcut: HAP frames are GPU-compressed (DXT) and would need a CPU decoder; add one if agents need HAP references
        return Err("HAP movies can't be looked at yet: an H.264 or ProRes copy of it can".into());
    }
    let count = count.max(1);
    let times: Vec<f64> = if times.is_empty() { (0..count).map(|k| (k as f64 + 0.5) / count as f64 * length).collect() } else { times.to_vec() };
    // the frame showing at each time, from where it starts
    let picks = times.iter().map(|&t| {
        let i = media.frame_at(t.clamp(0.0, (length - 1e-6).max(0.0)));
        (i, if i == 0 { 0.0 } else { media.ends[i - 1] })
    });
    let [w, h] = media.size;
    let frames = match &media.video {
        #[cfg(any(target_os = "macos", windows))]
        Some(clip) => {
            let mut bgra = Vec::new();
            picks
                .map(|(_, start)| {
                    let at = crate::video::Decoder::new(clip, start)?.next(&mut bgra).ok_or("no frame there")?;
                    let rgba = bgra.as_chunks::<4>().0.iter().flat_map(|&[b, g, r, a]| [r, g, b, a]).collect();
                    image::RgbaImage::from_raw(w, h, rgba).map(|p| (at, p)).ok_or_else(|| "a frame came out another size than the movie".to_string())
                })
                .collect::<Result<_, String>>()?
        }
        _ => picks.map(|(i, start)| (start, image::RgbaImage::from_raw(w, h, media.frames[i].clone()).unwrap_or_default())).collect(),
    };
    Ok(Sampled { size: media.size, length, frames })
}

/// A still's pixels, upright: a camera keeps a photo as its sensor saw it and says in its EXIF
/// orientation which way up it was held, so it's turned (or mirrored) as that says.
fn still(path: &Path) -> Result<image::RgbaImage, String> {
    #[cfg(target_os = "macos")]
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif")) {
        return system_still(path);
    }
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path).map_err(|e| e.to_string())?.into_decoder().map_err(|e| e.to_string())?;
    // asked before the pixels are: decoding takes the decoder. A file that doesn't say is upright
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    picture.apply_orientation(orientation);
    Ok(picture.into_rgba8())
}

/// A still the `image` crate doesn't read (HEIC, HEIF), decoded by macOS through ImageIO:
/// upright (its orientation applied), in sRGB (a phone's photos are in Display P3), at most
/// [`MAX_SIDE`] a side (a bigger one comes in smaller, not refused).
#[cfg(target_os = "macos")]
fn system_still(path: &Path) -> Result<image::RgbaImage, String> {
    use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFString, CFType, CFURL, CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB};
    use objc2_image_io::{CGImageSource, kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize};
    // ImageIO autoreleases as it goes, and a loading thread has no pool of its own
    objc2::rc::autoreleasepool(|_| {
        const UNREADABLE: &str = "macOS can't read this picture";
        let url = CFURL::from_file_path(path).ok_or(UNREADABLE)?;
        let source = unsafe { CGImageSource::with_url(&url, None) }.ok_or(UNREADABLE)?;
        // a "thumbnail" as big as the picture: the one call that hands it over turned as its
        // orientation says
        let (yes, most) = (CFBoolean::new(true), CFNumber::new_i32(MAX_SIDE as i32));
        let keys: [&CFString; 3] = unsafe { [kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize] };
        let values: [&CFType; 3] = [yes, yes, &most];
        let options = CFDictionary::from_slices(&keys, &values);
        let picture = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }.ok_or(UNREADABLE)?;
        let (w, h) = (CGImage::width(Some(&picture)), CGImage::height(Some(&picture)));
        // drawn into our own RGBA rows, top first: whatever layout and color space it has
        let mut pixels = vec![0u8; w * h * 4];
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or(UNREADABLE)?;
        let context = unsafe { CGBitmapContextCreate(pixels.as_mut_ptr().cast(), w, h, 8, w * 4, Some(&space), CGImageAlphaInfo::PremultipliedLast.0) }.ok_or(UNREADABLE)?;
        CGContext::draw_image(Some(&context), CGRect { origin: CGPoint { x: 0.0, y: 0.0 }, size: CGSize { width: w as f64, height: h as f64 } }, Some(&picture));
        drop(context);
        // the context's colors come multiplied by their alpha; a graph's pictures hold them plain
        for [r, g, b, a] in pixels.as_chunks_mut::<4>().0.iter_mut().filter(|p| p[3] != 255 && p[3] != 0) {
            for c in [r, g, b] {
                *c = (u32::from(*c) * 255 / u32::from(*a)).min(255) as u8;
            }
        }
        image::RgbaImage::from_raw(w as u32, h as u32, pixels).ok_or_else(|| UNREADABLE.to_string())
    })
}

fn sequence(folder: &Path) -> Result<Media, String> {
    let read = std::fs::read_dir(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let mut files: Vec<PathBuf> = read.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.is_file() && p.extension().is_some_and(|e| PICTURES.iter().any(|x| e.eq_ignore_ascii_case(x)))).collect();
    if files.is_empty() {
        return Err(format!("no images in {}", folder.display()));
    }
    files.sort_by_cached_key(|p| natural(&p.file_name().unwrap_or_default().to_string_lossy()));
    let mut media = Media::empty();
    // ponytail: one frame after the other on one thread; a pool if long sequences load slowly
    for (i, file) in files.iter().enumerate() {
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let picture = still(file).map_err(|e| format!("{name}: {e}"))?;
        media.push(picture, (i + 1) as f64 / SEQUENCE_FPS).map_err(|e| format!("{name}: {e}"))?;
    }
    Ok(media)
}

/// A file name to sort by, its numbers padded: "f2" before "f10", as people number frames.
fn natural(name: &str) -> String {
    let mut key = String::with_capacity(name.len() + 20);
    let mut digits = String::new();
    for c in name.chars().map(Some).chain([None]) {
        match c {
            Some(c) if c.is_ascii_digit() => digits.push(c),
            _ => {
                if !digits.is_empty() {
                    key.extend(std::iter::repeat_n('0', 20usize.saturating_sub(digits.len())));
                    key.push_str(&digits);
                    digits.clear();
                }
                key.extend(c);
            }
        }
    }
    key
}

/// A file loading on its own thread: empty until it's decoded (or failed).
pub type Loading = Arc<OnceLock<Result<Arc<Media>, String>>>;

/// The files Image nodes asked for, by path: each loads once, on a thread of its own, and
/// stays while anyone (the UI, a build) holds it.
#[derive(Default)]
pub struct Library {
    files: HashMap<String, Loading>,
}

impl Library {
    pub fn get(&mut self, path: &str) -> Loading {
        let file = PathBuf::from(path);
        self.load(path.to_string(), move || Media::load(&file))
    }

    /// A Text node's glyphs and layout ([`crate::text`]) as a still, in the bundled fonts and
    /// `font` (a file), on a thread of their own as a file loads.
    pub fn text(&mut self, text: &str, font: Option<&str>) -> Loading {
        let (words, file) = (text.to_string(), font.map(str::to_string));
        // apart from every path: none starts with a nul
        self.load(format!("\0{}\0{text}", font.unwrap_or_default()), move || crate::text::set(&words, file.as_deref()).and_then(Media::still))
    }

    /// A Live node's source (`ndi:<name>`, `spout:<name>`): connected on a thread of its own;
    /// the media is its first frame, and its reader runs while anyone holds it.
    pub fn live(&mut self, source: &str) -> Loading {
        let source = source.to_string();
        // apart from every path: none starts with a nul
        self.load(format!("\0live\0{source}"), move || crate::input::open(&source))
    }

    fn load(&mut self, key: String, make: impl FnOnce() -> Result<Media, String> + Send + 'static) -> Loading {
        // what nobody holds any more goes
        self.files.retain(|_, l| Arc::strong_count(l) > 1);
        if let Some(l) = self.files.get(&key) {
            return l.clone();
        }
        let loading = Loading::default();
        let into = loading.clone();
        if let Err(e) = std::thread::Builder::new().name("velo media".into()).spawn(move || {
            let _ = into.set(make().map(Arc::new));
        }) {
            let _ = loading.set(Err(e.to_string()));
        }
        self.files.insert(key, loading.clone());
        loading
    }
}

/// An Image node's picture as a graph plays it: the texture its shader reads, the frame on it.
pub struct Playing {
    /// The engine node, and its `speed` parameter's index.
    pub node: usize,
    pub speed: Option<usize>,
    /// Its `loop_start` and `loop_end` parameters' indices: the part of the clip that loops.
    pub range: Option<(usize, usize)>,
    pub media: Arc<Media>,
    pub texture: wgpu::Texture,
    shown: usize,
    /// Seconds into the clip (at its speed).
    at: f64,
    /// A movie's reader, on its own thread.
    reader: Option<(Arc<Reader>, std::thread::Thread)>,
    /// A live source's frame, swapped out of its mailbox.
    scratch: Vec<u8>,
}

/// What a movie's reader and the render thread share: the frame wanted next, the last one
/// read (swapped in, never copied), stop.
#[derive(Default)]
struct Reader {
    want: AtomicUsize,
    /// The frame, its texture's blocks, not uploaded yet.
    ready: Mutex<(usize, Vec<u8>, bool)>,
    stop: AtomicBool,
}

impl Playing {
    /// A texture as big as the file's frames, with the first on it; a movie's reader started.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, node: usize, speed: Option<usize>, media: Arc<Media>) -> Self {
        // a movie's frames go up as they are (whole 4×4 blocks); a picture's as display values
        // (the graph holds them so; the output decodes for sRGB)
        let (format, [width, height]) = match (&media.movie, &media.video) {
            // ponytail: a size that isn't a multiple of 4 shows the encoder's padding (up to 3 px)
            (Some(m), _) => (m.format.texture(), Movie::blocks(media.size)),
            // the system's frames come in BGRA
            (None, Some(_)) => (wgpu::TextureFormat::Bgra8Unorm, media.size),
            // so do a live source's
            (None, None) if media.live.is_some() => (wgpu::TextureFormat::Bgra8Unorm, media.size),
            (None, None) => (wgpu::TextureFormat::Rgba8Unorm, media.size),
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut playing = Self { node, speed, range: None, media, texture, shown: 0, at: 0.0, reader: None, scratch: Vec::new() };
        // a movie's first frame now, the rest on the reader's thread as they come due
        let mut first = Vec::new();
        match (&playing.media.movie, &playing.media.video) {
            (Some(movie), _) => {
                if let Ok(mut file) = std::fs::File::open(&movie.path)
                    && movie.read(&mut file, 0, &mut Vec::new(), &mut first).is_ok()
                {
                    playing.upload_staged(device, queue, &first);
                }
            }
            #[cfg(any(target_os = "macos", windows))]
            (None, Some(clip)) => {
                if let Ok(mut d) = crate::video::Decoder::new(clip, 0.0)
                    && d.next(&mut first).is_some()
                {
                    playing.upload_staged(device, queue, &first);
                }
            }
            _ => {
                playing.upload_staged(device, queue, &playing.media.frames[0]);
                return playing;
            }
        }
        if playing.media.count() > 1 {
            let (shared, media) = (Arc::new(Reader::default()), playing.media.clone());
            let thread = {
                let shared = shared.clone();
                std::thread::Builder::new().name("velo movie".into()).spawn(move || read(&media, &shared))
            };
            playing.reader = thread.ok().map(|t| (shared, t.thread().clone()));
        }
        playing
    }

    /// `dt` seconds later at `speed` (1: as the file plays), looping the part `window` (from,
    /// to: fractions of the clip): the frame due, if it changed. A movie's frame is read ahead,
    /// for the next frame; one that isn't read yet waits a frame.
    pub fn step(&mut self, queue: &wgpu::Queue, dt: f32, speed: f32, window: (f32, f32)) {
        // a live source's newest frame, if one came since the last: never waited for
        if let Some(feed) = self.media.live.clone() {
            if feed.take(&mut self.scratch) {
                self.upload(queue, &self.scratch);
            }
            return;
        }
        let (from, to) = (f64::from(window.0), f64::from(window.1));
        if self.media.count() < 2 {
            return;
        }
        let step = f64::from(dt) * f64::from(speed.max(0.0));
        self.at += step;
        match &self.reader {
            Some((reader, thread)) => {
                let next = self.media.frame_in(self.at + step, from, to);
                if reader.want.swap(next, Relaxed) != next {
                    thread.unpark();
                }
                // never waits: the reader holds the lock only to swap a frame in
                if let Ok(mut ready) = reader.ready.try_lock()
                    && ready.2
                {
                    ready.2 = false;
                    self.upload(queue, &ready.1);
                }
            }
            None => {
                let due = self.media.frame_in(self.at, from, to);
                if due != self.shown {
                    self.shown = due;
                    self.upload(queue, &self.media.frames[due]);
                }
            }
        }
    }

    /// A frame's bytes a row, and how many rows.
    fn rows(&self) -> (usize, u32) {
        let size = self.texture.size();
        match &self.media.movie {
            Some(m) => ((size.width / 4) as usize * m.format.block(), size.height / 4),
            None => (size.width as usize * 4, size.height),
        }
    }

    /// A frame from the worker (a picture, a movie's first): copied into a buffer of its own
    /// there, so the queue, which the render thread takes every frame, is held only to send the
    /// copy and never while a 4K picture is copied.
    fn upload_staged(&self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) {
        let (row, rows) = self.rows();
        let Some(bytes) = bytes.get(..row * rows as usize) else { return };
        let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some("image upload"), size: (padded * rows as usize) as u64, usage: wgpu::BufferUsages::COPY_SRC, mapped_at_creation: true });
        if let Ok(mut view) = buffer.slice(..).get_mapped_range_mut() {
            for (r, line) in bytes.chunks_exact(row).enumerate() {
                view.slice(r * padded..r * padded + row).copy_from_slice(line);
            }
        }
        buffer.unmap();
        let mut encoder = device.create_command_encoder(&Default::default());
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded as u32), rows_per_image: Some(rows) };
        encoder.copy_buffer_to_texture(wgpu::TexelCopyBufferInfo { buffer: &buffer, layout }, self.texture.as_image_copy(), self.texture.size());
        queue.submit([encoder.finish()]);
    }

    fn upload(&self, queue: &wgpu::Queue, bytes: &[u8]) {
        let size = self.texture.size();
        let (row, rows) = self.rows();
        let Some(bytes) = bytes.get(..row * rows as usize) else { return };
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row as u32), rows_per_image: Some(rows) };
        queue.write_texture(self.texture.as_image_copy(), bytes, layout, size);
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        if let Some((reader, thread)) = &self.reader {
            reader.stop.store(true, Relaxed);
            thread.unpark();
        }
    }
}

/// A movie's reader: the frame wanted, read and unpacked into a buffer of its own, then
/// swapped in for the render thread.
fn read(media: &Media, shared: &Reader) {
    #[cfg(any(target_os = "macos", windows))]
    if let Some(clip) = &media.video {
        return read_video(clip, shared);
    }
    let Some(movie) = &media.movie else { return };
    let Ok(mut file) = std::fs::File::open(&movie.path) else { return };
    let (mut packed, mut blocks, mut done) = (Vec::new(), Vec::new(), 0);
    while !shared.stop.load(Relaxed) {
        let want = shared.want.load(Relaxed);
        if want == done {
            std::thread::park_timeout(std::time::Duration::from_millis(100));
            continue;
        }
        done = want;
        // a frame that won't read is skipped: the last one stays up
        if movie.read(&mut file, want, &mut packed, &mut blocks).is_err() {
            continue;
        }
        let Ok(mut ready) = shared.ready.lock() else { return };
        std::mem::swap(&mut ready.1, &mut blocks);
        (ready.0, ready.2) = (want, true);
    }
}

/// A system-decoded movie's reader: frames come in order, so the one wanted is read up to;
/// one behind what's been read (the loop starting over) or far ahead starts reading there.
#[cfg(any(target_os = "macos", windows))]
fn read_video(clip: &Clip, shared: &Reader) {
    use crate::video::Decoder;
    let Ok(mut decoder) = Decoder::new(clip, 0.0) else { return };
    // frame 0 is up already (Playing::new)
    let (mut frame, mut next, mut done) = (Vec::new(), 0usize, 0usize);
    let far = (clip.fps.ceil() as usize * 2).max(2);
    while !shared.stop.load(Relaxed) {
        let want = shared.want.load(Relaxed);
        if want == done {
            std::thread::park_timeout(std::time::Duration::from_millis(100));
            continue;
        }
        done = want;
        if want < next || want > next + far {
            match Decoder::new(clip, want as f64 / clip.fps) {
                Ok(d) => (decoder, next) = (d, want),
                Err(_) => continue,
            }
        }
        // ponytail: a loop starting over opens a new reader (a few ms); two readers
        // alternating would hide it
        let mut got = false;
        while let Some(t) = decoder.next(&mut frame) {
            let index = (t * clip.fps).round() as usize;
            next = index + 1;
            if index >= want {
                got = true;
                break;
            }
        }
        if !got {
            continue;
        }
        let Ok(mut ready) = shared.ready.lock() else { return };
        std::mem::swap(&mut ready.1, &mut frame);
        (ready.0, ready.2) = (want, true);
    }
}

/// How many Image nodes a Media List shows its files through: the turn before (fading out),
/// this one, the next.
pub const SLOTS: usize = 3;

/// Why the files a Media List couldn't play didn't, by path: until one plays again.
static FAILED: Mutex<std::collections::BTreeMap<String, String>> = Mutex::new(std::collections::BTreeMap::new());

/// Why a Media List's file didn't play on its last turn, if it didn't.
pub fn failed(path: &str) -> Option<String> {
    FAILED.lock().ok()?.get(path).cloned()
}

/// What a Media List's loader is asked for: the files due, the most pressing first, and the
/// one it's loading.
#[derive(Default)]
struct Asked {
    need: Vec<usize>,
    loading: Option<usize>,
    stop: bool,
}

#[derive(Default)]
struct Loader {
    asked: Mutex<Asked>,
    wake: std::sync::Condvar,
}

impl Loader {
    /// The next file to load, once one is due; none once the reel is gone.
    fn next(&self) -> Option<usize> {
        let mut asked = self.asked.lock().ok()?;
        loop {
            if asked.stop {
                return None;
            }
            if !asked.need.is_empty() {
                let file = asked.need.remove(0);
                asked.loading = Some(file);
                return Some(file);
            }
            asked = self.wake.wait(asked).ok()?;
        }
    }
}

/// A Media List as it plays: its files shown through [`SLOTS`] Image nodes, turn k's on slot
/// k % 3 (medialist.wgsl), loaded on a thread of the list's own, the most pressing first.
/// Streamed (its `load` parameter at 0), only this turn's file, the next and the one fading out
/// are in, and each plays from its start on its turn. In memory (1), every file loads and
/// stays: a turn never waits, at the memory all of them take, and each plays on from where its
/// last turn left it. A movie's turn lasts `movie_every` (beats or seconds as the clock goes),
/// a picture's `every`; a movie shorter than its turn moves on to the next file once it has
/// played through (`short_movie` at 0), or repeats until the turn is over (1). On Hits (`clock`
/// at 2) no clock moves it: a turn lasts until `hit` next rises past a half ([`Hits`]), a movie
/// repeating meanwhile.
pub struct Reel {
    /// The Media List's node id and engine node, its order, clock, every, fade, load,
    /// movie_every, short_movie and hit parameters' indices, and those of what it tells its
    /// shader: the slot showing and how much of the one before is still in.
    id: String,
    pub node: usize,
    params: [usize; 8],
    /// The hits counted so far: the turn it's on, on Hits.
    hits: Hits,
    /// What the turns that ended early (a short movie played through) have put the list ahead
    /// of its clock by, in the clock's own beats or seconds.
    ahead: f32,
    /// The turn it's on, and where its movie was (seconds played) when the turn began.
    began: (i64, f64),
    shows: [usize; 2],
    /// Its slots' engine nodes, and what each shows: the turn, its file, whether that was in.
    slots: [usize; SLOTS],
    bound: [Option<(i64, usize, bool)>; SLOTS],
    files: Vec<String>,
    /// Which files are movies.
    movie: Vec<bool>,
    /// Each file in, by index; one that failed isn't asked for again until it's due anew.
    cache: Vec<Option<Playing>>,
    broken: Vec<bool>,
    loader: Arc<Loader>,
    done: std::sync::mpsc::Receiver<(usize, Result<Playing, String>)>,
}

impl Reel {
    /// The list `id`, engine node `node` running `shader` (medialist.wgsl, none without its
    /// parameters). Movies in HAP only with `bc` (BC textures on this GPU).
    #[allow(clippy::too_many_arguments)]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, id: String, node: usize, shader: &crate::ShaderNode, slots: [usize; SLOTS], files: Vec<String>, bc: bool) -> Option<Self> {
        let at = |param| crate::modulate::slot(shader, param).map(|s| s.0);
        let params = [at("order")?, at("clock")?, at("every")?, at("fade")?, at("load")?, at("movie_every")?, at("short_movie")?, at("hit")?];
        let shows = [at("_now")?, at("_fade")?];
        let movie = files.iter().map(|f| Path::new(f).extension().is_some_and(|e| MOVIES.iter().any(|m| e.eq_ignore_ascii_case(m)))).collect();
        let loader = Arc::new(Loader::default());
        let (answer, done) = std::sync::mpsc::channel();
        let (device, queue, shared, paths) = (device.clone(), queue.clone(), loader.clone(), files.clone());
        let _ = std::thread::Builder::new().name("velo media list".into()).spawn(move || {
            while let Some(file) = shared.next() {
                let path = &paths[file];
                let playing = open(&device, &queue, path, bc);
                if let Ok(mut failed) = FAILED.lock() {
                    match &playing {
                        Ok(_) => failed.remove(path),
                        Err(e) => failed.insert(path.clone(), e.clone()),
                    };
                }
                // answered with the lock held: the reel never asks again for what's on its way
                let Ok(mut asked) = shared.asked.lock() else { return };
                asked.loading = None;
                if answer.send((file, playing)).is_err() {
                    return;
                }
            }
        });
        let n = files.len();
        Some(Self { id, node, params, hits: Hits::default(), ahead: 0.0, began: (-1, 0.0), shows, slots, bound: [None; SLOTS], files, movie, cache: (0..n).map(|_| None).collect(), broken: vec![false; n], loader, done })
    }

    /// This frame, at `beats` and `time` (seconds): the turn, told to the shader with how much
    /// of the one before still shows, the files due asked for, those in bound to their slots,
    /// this turn's played on and the one before while it fades out. `value` is the list's
    /// parameters' values.
    #[allow(clippy::too_many_arguments)]
    pub fn step(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, runner: &mut crate::gpu::Runner, programs: &[crate::gpu::Program], value: impl Fn(usize) -> f32, beats: f32, time: f32, dt: f32) {
        let n = self.files.len();
        if n == 0 {
            return;
        }
        let [order, clock, every, fade, load, movie_every, short_movie, hit] = self.params.map(value);
        let (order, clock) = (order.round() as i32, clock.round() as i32);
        // counted whatever the clock: switched to Hits, it knows where `hit` stands
        self.hits.feed(hit, beats);
        // a movie's turn as long as a picture's unless it's given its own
        let every = every.max(0.01);
        let movie_every = if movie_every > 0.0 { movie_every.max(0.01) } else { every };
        let long = |movie: bool| if movie { movie_every } else { every };
        let (turn, k) = if clock == 2 {
            // on hits a turn is as long as the wait for the next: its fade is measured as if it
            // lasted `every` beats, and it then stays as it is
            (self.hits.count, ((beats - self.hits.at) / every).clamp(0.0, 1.0))
        } else {
            let t = if clock == 1 { time } else { beats }.max(0.0) + self.ahead;
            turn_at(t, n, order, |f| long(self.movie[f]))
        };
        let fading = fade > 0.0 && turn > 0 && k < fade;
        // the first part of each turn crossfades from the one before
        let before = if fading {
            let x = k / fade;
            1.0 - x * x * (3.0 - 2.0 * x)
        } else {
            0.0
        };
        runner.write_param(queue, self.node, self.shows[0], [turn.rem_euclid(SLOTS as i64) as f32, 0.0, 0.0, 0.0]);
        runner.write_param(queue, self.node, self.shows[1], [before, 0.0, 0.0, 0.0]);
        let file = |at: i64| pick(at, n, order);
        // the files due, the most pressing first: this turn's, the next, the one fading out,
        // then (in memory) every other
        let (mut due, mut wanted) = (Vec::new(), vec![false; n]);
        let rest = if load.round() as i32 == 1 { 0..n } else { 0..0 };
        for f in [turn, turn + 1].into_iter().chain(fading.then_some(turn - 1)).map(file).chain(rest) {
            if !std::mem::replace(&mut wanted[f], true) {
                due.push(f);
            }
        }
        if let Ok(mut asked) = self.loader.asked.lock() {
            for (f, playing) in self.done.try_iter() {
                match playing {
                    Ok(playing) if wanted[f] => self.cache[f] = Some(playing),
                    Ok(_) => {}
                    Err(_) => self.broken[f] = true,
                }
            }
            // what isn't due goes, and a broken file is tried again once it's due anew
            for f in (0..n).filter(|&f| !wanted[f]) {
                (self.cache[f], self.broken[f]) = (None, false);
            }
            let need: Vec<usize> = due.into_iter().filter(|&f| self.cache[f].is_none() && !self.broken[f] && asked.loading != Some(f)).collect();
            if need != asked.need {
                asked.need = need;
                self.loader.wake.notify_one();
            }
        }
        for at in [turn, turn + 1, turn - 1].into_iter().filter(|&at| at >= 0) {
            let (slot, f) = (at.rem_euclid(SLOTS as i64) as usize, file(at));
            let shown = self.cache[f].as_ref();
            if self.bound[slot] != Some((at, f, shown.is_some())) {
                runner.set_file(device, programs, self.slots[slot], shown.map(|p| p.texture.create_view(&Default::default())).as_ref());
                self.bound[slot] = Some((at, f, shown.is_some()));
            }
        }
        // this turn's file plays on, and the one fading out (once, if it's the same)
        let mut played = None;
        for f in [turn].into_iter().chain(fading.then_some(turn - 1)).map(file) {
            if played != Some(f)
                && let Some(playing) = &mut self.cache[f]
            {
                playing.step(queue, dt, 1.0, (0.0, 1.0));
            }
            played = Some(f);
        }
        // a movie shorter than its turn, played through: on to the next file now, the rest of
        // its turn skipped (unless it's set to repeat, it's the only file, or only a hit moves
        // the list on)
        let f = file(turn);
        let at = self.cache[f].as_ref().map_or(0.0, |p| p.at);
        if self.began.0 != turn {
            self.began = (turn, at);
        } else if short_movie.round() as i32 == 0
            && clock != 2
            && n > 1
            && self.movie[f]
            && let Some(length) = self.cache[f].as_ref().and_then(|p| p.media.ends.last().copied())
            && length > 0.0
            && at - self.began.1 >= length - f64::from(dt)
        {
            self.ahead += (1.0 - k) * long(true) + 1e-3;
        }
    }

    /// What `old`, the same list in the graph this one replaces, has in or loading, carried
    /// over (bound to this one's slots on its first step): a rebuild never shows its files black.
    pub fn adopt(&mut self, old: &mut Reel) {
        if (&old.id, &old.files) == (&self.id, &self.files) {
            std::mem::swap(&mut self.cache, &mut old.cache);
            std::mem::swap(&mut self.broken, &mut old.broken);
            std::mem::swap(&mut self.loader, &mut old.loader);
            std::mem::swap(&mut self.done, &mut old.done);
            (self.hits, self.ahead, self.began) = (old.hits, old.ahead, old.began);
        }
    }

    /// How many of its files are in.
    #[cfg(test)]
    pub(crate) fn in_memory(&self) -> usize {
        self.cache.iter().flatten().count()
    }
}

impl Drop for Reel {
    fn drop(&mut self) {
        if let Ok(mut asked) = self.loader.asked.lock() {
            asked.stop = true;
        }
        self.loader.wake.notify_one();
    }
}

/// A Text List as it plays: its turns counted and each one's line picked here, as a Media List
/// picks its files (at random is the same deck, shuffled anew every round, which its shader,
/// working every pixel out alone, can't deal), then told to the shader (textlist.wgsl).
pub struct Lines {
    pub node: usize,
    /// Its order, clock and every parameters' indices, and those of what it tells its shader:
    /// the line showing, the one before it, how far through the turn it is.
    params: [usize; 3],
    shows: [usize; 3],
    /// How many items it has.
    count: usize,
}

impl Lines {
    /// Engine node `node` running `shader` (textlist.wgsl, none without its parameters), a
    /// list of `count` items.
    pub fn new(node: usize, shader: &crate::ShaderNode, count: usize) -> Option<Self> {
        let at = |param| crate::modulate::slot(shader, param).map(|s| s.0);
        Some(Self { node, params: [at("order")?, at("clock")?, at("every")?], shows: [at("_now")?, at("_before")?, at("_turn")?], count })
    }

    /// This frame, at `beats` and `time` (seconds). `value` is the list's parameters' values.
    pub fn step(&self, queue: &wgpu::Queue, runner: &crate::gpu::Runner, value: impl Fn(usize) -> f32, beats: f32, time: f32) {
        let [order, clock, every] = self.params.map(value);
        let (line, before, k) = line_at(if clock.round() as i32 == 1 { time } else { beats }, self.count, order.round() as i32, every);
        for (param, told) in self.shows.into_iter().zip([line as f32, before.map_or(-1.0, |b| b as f32), k]) {
            runner.write_param(queue, self.node, param, [told, 0.0, 0.0, 0.0]);
        }
    }
}

/// What a Text List of `n` items shows at `t` (beats or seconds), a new one every `every`: the
/// line, the one before it (none on the first turn) and how far through its turn it is (0 to 1).
fn line_at(t: f32, n: usize, order: i32, every: f32) -> (usize, Option<usize>, f32) {
    // a clock moved back (a resync, a tap) doesn't count below the first
    let (turn, k) = turn_at(t.max(0.0), n, order, |_| every.max(0.01));
    (pick(turn, n, order), (turn > 0).then(|| pick(turn - 1, n, order)), k)
}

/// A Media List's file, playing: on a texture of its own, a still's pixels only there.
fn open(device: &wgpu::Device, queue: &wgpu::Queue, path: &str, bc: bool) -> Result<Playing, String> {
    let media = Media::load(Path::new(path))?;
    let name = Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    if media.model.is_some() {
        return Err(format!("{name}: a 3D model isn't a picture"));
    }
    if media.movie.is_some() && !bc {
        return Err(format!("{name}: HAP needs BC textures, which this GPU hasn't"));
    }
    // the reel plays it itself: no engine node, no speed
    let mut playing = Playing::new(device, queue, 0, None, Arc::new(media));
    if playing.media.count() < 2
        && let Some(media) = Arc::get_mut(&mut playing.media)
    {
        media.frames = Vec::new();
    }
    Ok(playing)
}

/// The times a Media List's `hit` has risen past a half (a Kick mapped onto it: its kicks),
/// and the beat the last one came on.
#[derive(Clone, Copy, Default)]
struct Hits {
    count: i64,
    at: f32,
    /// Whether `hit` was past a half the last time it was looked at: it counts again once it
    /// has come back down.
    high: bool,
}

impl Hits {
    /// `hit` as it stands at `beats`.
    fn feed(&mut self, hit: f32, beats: f32) {
        let high = hit > 0.5;
        if high && !self.high {
            (self.count, self.at) = (self.count + 1, beats);
        }
        self.high = high;
    }
}

/// How many turns a round of a list of `n` takes: each file once, or there and back.
fn period(n: usize, order: i32) -> usize {
    match order {
        _ if n < 2 => 1,
        2 => 2 * n - 2,
        _ => n,
    }
}

/// The files round `round` of a list of `n` shows, in turn: in order, at random (each once a
/// round, never the one just before) or back and forth.
fn round_files(round: usize, n: usize, order: i32) -> Vec<usize> {
    match order {
        _ if n < 2 => vec![0],
        // two at random, never the one before, are in turn: the last arm
        1 if n > 2 => {
            let mut files = deck(round, n);
            // a round doesn't start on the file the one before ended on: its first two swap
            // (the last of a round of three or more never moves)
            if round > 0 && files[0] == deck(round - 1, n)[n - 1] {
                files.swap(0, 1);
            }
            files
        }
        2 => (0..2 * n - 2).map(|s| if s >= n { 2 * n - 2 - s } else { s }).collect(),
        _ => (0..n).collect(),
    }
}

/// The file turn `at` shows.
fn pick(at: i64, n: usize, order: i32) -> usize {
    let (at, m) = (at.max(0) as usize, period(n, order));
    round_files(at / m, n, order)[at % m]
}

/// The turn the clock is on at `t` (beats or seconds), and how far through it it is (0 to 1),
/// each file's turn as long as `long` says. A round lasts as long whichever it is (the same
/// files), so it's found at once, then the turn in it.
fn turn_at(t: f32, n: usize, order: i32, long: impl Fn(usize) -> f32) -> (i64, f32) {
    let m = period(n, order);
    let length: f32 = round_files(0, n, order).into_iter().map(&long).sum();
    let round = (t / length).floor().max(0.0);
    let mut start = round * length;
    let files = round_files(round as usize, n, order);
    for (i, &f) in files.iter().enumerate() {
        let long = long(f);
        if t < start + long || i + 1 == files.len() {
            return ((round as usize * m + i) as i64, ((t - start) / long).clamp(0.0, 1.0));
        }
        start += long;
    }
    (0, 0.0)
}

/// Round `round`'s files shuffled, the same every time it's asked.
fn deck(round: usize, n: usize) -> Vec<usize> {
    let mut deck: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        deck.swap(i, (mix(mix(round as u64 ^ shuffle_seed()) ^ i as u64) % (i as u64 + 1)) as usize);
    }
    deck
}

/// Where this run's shuffles start: new each time Velo starts, so a show opened again doesn't
/// deal the same words and pictures in the same order; the same for the whole run, so a list
/// rebuilt mid-show carries on where it was. `VELO_SHUFFLE_SEED` pins it (tests, screenshots).
fn shuffle_seed() -> u64 {
    static SEED: OnceLock<u64> = OnceLock::new();
    *SEED.get_or_init(|| {
        std::env::var("VELO_SHUFFLE_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or_else(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64)
        })
    })
}

/// splitmix64's finalizer: a well mixed hash of x.
fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_lists_turns_pick_its_files() {
        let picks = |order, n| (0..24).map(|at| pick(at, n, order)).collect::<Vec<_>>();
        assert_eq!(picks(0, 3)[..7], [0, 1, 2, 0, 1, 2, 0], "in order");
        assert_eq!(picks(2, 3)[..9], [0, 1, 2, 1, 0, 1, 2, 1, 0], "back and forth");
        assert_eq!(picks(1, 2)[..4], [0, 1, 0, 1], "two at random: in turn");
        assert!(picks(1, 1).iter().all(|&k| k == 0) && picks(2, 1).iter().all(|&k| k == 0), "one file: always it");
        for n in [3, 4, 7] {
            let shown: Vec<usize> = (0..60).map(|at| pick(at, n, 1)).collect();
            assert!(shown.windows(2).all(|w| w[0] != w[1]), "{n} at random: never twice running ({shown:?})");
            for round in shown.chunks(n).filter(|r| r.len() == n) {
                let mut sorted = round.to_vec();
                sorted.sort();
                assert_eq!(sorted, (0..n).collect::<Vec<_>>(), "each once a round");
            }
        }
        assert_ne!(picks(1, 7), picks(0, 7), "at random isn't in order");
    }

    #[test]
    fn a_hit_counts_each_rise_past_a_half() {
        let mut hits = Hits::default();
        // a frame a beat: nothing, a kick and its tail, a wobble under a half, a kick held, one more
        let counted: Vec<i64> = [0.0, 0.2, 1.0, 0.8, 0.6, 0.4, 0.45, 0.1, 0.9, 0.9, 0.5, 0.51]
            .iter()
            .enumerate()
            .map(|(beat, &hit)| {
                hits.feed(hit, beat as f32);
                hits.count
            })
            .collect();
        assert_eq!(counted, [0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2, 3]);
        assert_eq!(hits.at, 11.0, "the beat the last one came on");
    }

    #[test]
    fn a_text_list_at_random_is_a_deck_shuffled_every_round() {
        // five lines, a new one every 2 beats, looked at in the middle of each turn
        let shown: Vec<(usize, Option<usize>, f32)> = (0..100).map(|turn| line_at(turn as f32 * 2.0 + 1.0, 5, 1, 2.0)).collect();
        let lines: Vec<usize> = shown.iter().map(|s| s.0).collect();
        let rounds: Vec<&[usize]> = lines.chunks(5).collect();
        for round in &rounds {
            let mut sorted = round.to_vec();
            sorted.sort();
            assert_eq!(sorted, [0, 1, 2, 3, 4], "each line once a round");
        }
        assert!(lines.windows(2).all(|w| w[0] != w[1]), "never the same twice running");
        // a stride through five lines has 20 ways round, each a line apart from the last by the
        // same step: a shuffle has more, and rounds that are no stride at all
        let ways: std::collections::BTreeSet<&[usize]> = rounds.iter().copied().collect();
        let stride = |r: &[usize]| r.windows(2).map(|w| (w[1] + 5 - w[0]) % 5).collect::<std::collections::BTreeSet<_>>().len() == 1;
        assert!(ways.len() > 15 && rounds.iter().any(|r| !stride(r)), "{} ways round in 20 rounds", ways.len());
        // the line before is the last turn's, none on the first; halfway through each turn
        assert_eq!(shown[0].1, None);
        assert!(shown.windows(2).all(|w| w[1].1 == Some(w[0].0)) && shown.iter().all(|s| (s.2 - 0.5).abs() < 1e-3));
        // the other orders, and a clock moved back, as they were
        let order = |order| (0..7).map(|turn| line_at(turn as f32 * 2.0, 3, order, 2.0).0).collect::<Vec<_>>();
        assert_eq!((order(0), order(2)), (vec![0, 1, 2, 0, 1, 2, 0], vec![0, 1, 2, 1, 0, 1, 2]));
        assert_eq!((line_at(-3.0, 3, 0, 2.0), line_at(5.0, 0, 1, 2.0).0), ((0, None, 0.0), 0), "below the first turn, and a list of nothing");
    }

    #[test]
    fn a_movies_turn_lasts_its_own() {
        // three files in order, the second a movie: 4, 10, 4 beats
        let long = |f: usize| if f == 1 { 10.0 } else { 4.0 };
        let turn = |t| turn_at(t, 3, 0, long);
        assert_eq!(turn(1.0), (0, 0.25));
        assert_eq!(turn(4.0), (1, 0.0), "the movie from 4");
        assert_eq!(turn(9.0), (1, 0.5));
        assert_eq!(turn(14.0), (2, 0.0), "the third from 14");
        assert_eq!(turn(18.0), (3, 0.0), "round again at 18");
        assert_eq!(turn(18.0 * 10.0 + 5.0), (31, 0.1), "found at once however far on");
        // all as long: a turn every so many beats, as ever
        assert!((0..40).all(|b| turn_at(b as f32 + 0.5, 5, 0, |_| 4.0).0 == (b / 4) as i64));
        // at random, each round as long: the same files, shuffled
        let random: Vec<i64> = [0.0, 4.5, 18.5, 40.0].iter().map(|&t| turn_at(t, 3, 1, long).0).collect();
        assert!(random.windows(2).all(|w| w[0] <= w[1]) && random[3] >= 6, "{random:?}");
    }

    /// The H.264 fixture: red | green over blue, then green, blue, white, at 10 fps (video.rs).
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn an_agent_gets_a_movies_frames_where_it_asks() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/h264.mp4");
        let spread = sample(&path, &[], 4).unwrap();
        assert_eq!((spread.size, spread.frames.len()), ([96, 48], 4));
        assert!((spread.length - 0.4).abs() < 1e-6);
        let starts: Vec<f64> = spread.frames.iter().map(|f| (f.0 * 10.0).round() / 10.0).collect();
        assert_eq!(starts, [0.0, 0.1, 0.2, 0.3]);
        // RGBA now: the third frame is blue, the second green
        assert_eq!(spread.frames[2].1.get_pixel(48, 24).0[..3].iter().map(|&c| c > 128).collect::<Vec<_>>(), [false, false, true]);
        assert_eq!(spread.frames[1].1.get_pixel(48, 24).0[..3].iter().map(|&c| c > 128).collect::<Vec<_>>(), [false, true, false]);
        // times past the end hold the last frame
        let asked = sample(&path, &[0.25, 99.0], 0).unwrap();
        assert_eq!(asked.frames.iter().map(|f| (f.0 * 10.0).round() / 10.0).collect::<Vec<_>>(), [0.2, 0.3]);
    }

    #[test]
    fn a_model_is_points_not_frames() {
        let folder = dir("model");
        let path = folder.join("tri.obj");
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap();
        let media = Media::load(&path).unwrap();
        assert_eq!((media.model.as_ref().map(|m| m.points.len()), media.count()), (Some(3), 0));
        assert!(sample(&path, &[], 2).is_err_and(|e| e.contains("model")), "an agent isn't handed frames that aren't there");
        std::fs::write(&path, "nothing here\n").unwrap();
        assert!(Media::load(&path).unwrap_err().contains("no points"));
    }

    #[test]
    fn a_hap_movie_says_it_cant_be_looked_at() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/hap1.mov");
        assert!(sample(&path, &[], 2).is_err_and(|e| e.contains("HAP")));
    }

    #[test]
    fn a_part_of_the_clip_loops_on_its_own() {
        // ten frames a tenth of a second each
        let media = Media { size: [1, 1], frames: vec![Vec::new(); 10], ends: (1..=10).map(|k| f64::from(k) / 10.0).collect(), movie: None, video: None, live: None, model: None };
        assert_eq!((media.frame_at(0.05), media.frame_at(1.05)), (0, 0), "the whole clip loops");
        // from 0.4 to 0.6 of it: frames 4 and 5, again and again
        let shown: Vec<usize> = [0.0, 0.05, 0.1, 0.15, 0.2, 0.25].iter().map(|&t| media.frame_in(t, 0.4, 0.6)).collect();
        assert_eq!(shown, [4, 4, 5, 5, 4, 4]);
        assert_eq!(media.frame_in(0.3, 0.6, 0.4), media.frame_in(0.3, 0.4, 0.6), "either way round");
        assert_eq!((media.frame_in(5.0, 0.7, 0.7), media.frame_in(1.0, 1.0, 1.0)), (7, 9), "a part shorter than a frame holds it");
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("velo-media-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn flat(rgb: [u8; 3], [w, h]: [u32; 2]) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
    }

    #[test]
    fn a_folder_plays_its_frames_in_number_order() {
        let d = dir("seq");
        // written out of order, unpadded: f2 comes before f10
        for (name, red) in [("f10.png", 30), ("f1.png", 10), ("f2.png", 20), ("notes.txt", 0)] {
            if name.ends_with(".png") {
                flat([red, 0, 0], [4, 2]).save(d.join(name)).unwrap();
            } else {
                std::fs::write(d.join(name), "not a frame").unwrap();
            }
        }
        let m = Media::load(&d).unwrap();
        assert_eq!(m.size, [4, 2]);
        assert_eq!(m.frames.iter().map(|f| f[0]).collect::<Vec<_>>(), [10, 20, 30]);
        assert!((m.length() - 3.0 / SEQUENCE_FPS).abs() < 1e-9);
        assert_eq!(m.frame_at(0.0), 0);
        assert_eq!(m.frame_at(1.5 / SEQUENCE_FPS), 1);
        assert_eq!(m.frame_at(2.5 / SEQUENCE_FPS), 2);
        assert_eq!(m.frame_at(3.5 / SEQUENCE_FPS), 0, "it loops");
        // a frame of another size is refused, an empty folder too
        flat([0, 0, 0], [8, 8]).save(d.join("f3.png")).unwrap();
        assert!(Media::load(&d).unwrap_err().contains("f3.png"));
        assert!(Media::load(&dir("empty")).unwrap_err().contains("no images"));
    }

    #[test]
    fn a_gif_plays_at_its_delays() {
        let d = dir("gif");
        let path = d.join("loop.gif");
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(std::fs::File::create(&path).unwrap());
            let frames = [(255, 100), (0, 300), (128, 0)].map(|(red, ms)| image::Frame::from_parts(flat([red, 0, 0], [6, 4]), 0, 0, image::Delay::from_numer_denom_ms(ms, 1)));
            encoder.encode_frames(frames).unwrap();
        }
        let m = Media::load(&path).unwrap();
        assert_eq!((m.size, m.frames.len()), ([6, 4], 3));
        // 100 ms, 300 ms, then 0 ms played as 100 ms
        assert!((m.length() - 0.5).abs() < 1e-9, "{}", m.length());
        assert_eq!([0.05, 0.2, 0.45, 0.55].map(|t| m.frame_at(t)), [0, 1, 2, 0]);
        assert!(m.frames[1][0] < 20 && m.frames[0][0] > 200, "the frames' own pixels");
    }

    #[test]
    fn a_still_is_one_frame_and_errors_name_the_file() {
        let d = dir("still");
        image::DynamicImage::from(flat([1, 2, 3], [3, 3])).to_rgb8().save(d.join("still.jpg")).unwrap();
        let m = Media::load(&d.join("still.jpg")).unwrap();
        assert_eq!((m.frames.len(), m.length(), m.frame_at(12.0)), (1, 0.0, 0));
        std::fs::write(d.join("broken.png"), "nope").unwrap();
        assert!(Media::load(&d.join("broken.png")).unwrap_err().contains("broken.png"));
        assert!(Media::load(&d.join("missing.png")).is_err());
    }

    /// Whether the still `m` is red (not blue) at x, y.
    fn red(m: &Media, x: usize, y: usize) -> bool {
        let p = &m.frames[0][(y * m.size[0] as usize + x) * 4..][..4];
        p[0] > 180 && p[2] < 80
    }

    #[test]
    fn a_photo_shows_upright_as_its_exif_says() {
        use image::ImageEncoder;
        let d = dir("exif");
        // red on the left, blue on the right, as the sensor saw it
        let picture = image::RgbImage::from_fn(16, 8, |x, _| image::Rgb(if x < 8 { [255, 0, 0] } else { [0, 0, 255] }));
        let load = |orientation: u8| {
            let path = d.join(format!("{orientation}.jpg"));
            let mut encoder = image::codecs::jpeg::JpegEncoder::new(std::fs::File::create(&path).unwrap());
            // EXIF with one entry: orientation (0x0112), a short
            encoder.set_exif_metadata([b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0".as_slice(), &[orientation], &[0; 6]].concat()).unwrap();
            encoder.write_image(&picture, 16, 8, image::ExtendedColorType::Rgb8).unwrap();
            Media::load(&path).unwrap()
        };
        let m = load(1);
        assert!(m.size == [16, 8] && red(&m, 3, 4) && !red(&m, 12, 4), "upright: as it is");
        let m = load(6);
        assert!(m.size == [8, 16] && red(&m, 4, 3) && !red(&m, 4, 12), "a phone held upright: a quarter turn clockwise, its left on top");
        let m = load(3);
        assert!(m.size == [16, 8] && red(&m, 12, 4) && !red(&m, 3, 4), "upside down: half a turn");
        let m = load(8);
        assert!(m.size == [8, 16] && red(&m, 4, 12) && !red(&m, 4, 3), "a quarter turn the other way: its left at the bottom");
    }

    /// The fixture: red | blue, 64×48 as stored, its orientation saying a quarter turn clockwise
    /// (`sips -s format heic` of a JPEG with EXIF orientation 6).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_heic_loads_upright_through_the_system() {
        let m = Media::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/turned.heic")).unwrap();
        assert_eq!((m.size, m.count()), ([48, 64], 1));
        assert!(red(&m, 24, 12) && !red(&m, 24, 52), "its left on top");
        assert_eq!(m.frames[0][3], 255, "opaque");
        let d = dir("heic");
        std::fs::write(d.join("broken.heic"), "nope").unwrap();
        assert!(Media::load(&d.join("broken.heic")).unwrap_err().contains("broken.heic"));
    }

    #[test]
    fn the_library_loads_each_file_once() {
        let d = dir("library");
        flat([9, 9, 9], [2, 2]).save(d.join("a.png")).unwrap();
        let path = d.join("a.png").to_string_lossy().into_owned();
        let mut library = Library::default();
        let a = library.get(&path);
        assert!(Arc::ptr_eq(&a, &library.get(&path)), "held: the same load");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while a.get().is_none() && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(a.get().unwrap().as_ref().unwrap().size, [2, 2]);
        // the loader lets go of it once it's done
        while Arc::strong_count(&a) > 2 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        drop(a);
        library.get("/nowhere");
        assert_eq!(library.files.len(), 1, "a file nobody holds goes");
    }
}
