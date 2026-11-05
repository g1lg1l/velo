//! Pictures from files for Image and Video nodes (docs/rendering.md §4): a still, an animated
//! GIF or a folder of numbered frames (an image sequence), decoded on a thread of their own into
//! RGBA frames in memory; or a movie (HAP, or one the system plays: H.264, HEVC, on macOS ProRes too), whose
//! frames stay in the file and are read as they come due. A graph built while a file loads shows black there and is built again once it's in;
//! the render thread uploads each frame as it comes due ([`Playing`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock};

use crate::hap::{Format, Movie};

/// What an Image or Video node takes: files by extension.
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
        let picture = image::open(path).map_err(|e| format!("{}: {e}", name()))?.to_rgba8();
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
        let picture = image::open(file).map_err(|e| format!("{name}: {e}"))?.to_rgba8();
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

#[cfg(test)]
mod tests {
    use super::*;

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
