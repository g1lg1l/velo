//! The render thread (ARCHITECTURE.md §4, spike S1): its own GPU device, a frame loop paced by
//! the output's vsync (or 60 Hz without one), graphs installed whole at a frame boundary, the
//! preview read back for the UI, which has its own device (S1: sharing one with eframe crashes on
//! resize). The loop never allocates pixels, compiles or waits for another thread. Each frame it
//! also runs the beat clock and the mappings (docs/graph.md §5): the UI sends base values and
//! routes, the modulated values are computed here.
//!
//! It keeps a few scene graphs, by the keys the UI gave them (docs/scenes.md §3): the live one
//! goes out (the program); a launch waits for its quantize boundary, then cuts or crossfades;
//! the graph being edited renders too when it isn't live, for the preview only. The rest wait,
//! built, so launching them is instant.

use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use velo_core::{Layer, LayerBlend, MACROS, Quantize, Transition};

use crate::gpu::{Program, Runner};
use crate::launch::Show;
use crate::modulate::{Clock, MacroRoute, Macros, Modulator, Route};

/// The preview the UI gets: small, and at most this often.
pub const PREVIEW: [u32; 2] = [640, 360];
const PREVIEW_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const PREVIEW_EVERY: Duration = Duration::from_millis(16);
/// Each node's picture for the canvas' cards: this small, in a grid of this many, this often.
pub const THUMB: [u32; 2] = [128, 72];
pub const THUMBS: [u32; 2] = [12, 8];
const THUMBS_EVERY: Duration = Duration::from_millis(100);
/// How many frames a graph renders before its card's picture is taken: its feedback and its
/// files' first frames in.
const PEEK: u32 = 30;

/// Audio features as atomics: the analysis thread stores, the render thread loads, nobody waits.
/// In the order of `velo_audio::Features::globals`.
#[derive(Default)]
pub struct SharedFeatures([AtomicU32; 12]);

impl SharedFeatures {
    pub fn store(&self, values: [f32; 12]) {
        for (slot, v) in self.0.iter().zip(values) {
            slot.store(v.to_bits(), Relaxed);
        }
    }

    /// The latest values (what the UI's meters show, too).
    pub fn load(&self) -> [f32; 12] {
        std::array::from_fn(|i| f32::from_bits(self.0[i].load(Relaxed)))
    }
}

/// Hops of history in the audio texture (~2.7 s at 48 kHz), raw samples in its scope row, and
/// bands in its spectrum row.
pub const WAVE_COLUMNS: usize = 512;
/// The audio texture's rows: the hops' levels, the scope, the spectrum.
pub const WAVE_ROWS: usize = 3;
/// The spectrum row's range, its bands spaced evenly in octaves.
const LOWEST_HZ: f32 = 30.0;
const HIGHEST_HZ: f32 = 16_000.0;

/// The live audio for shaders that read `wave`: the last hops' levels, the last samples and the
/// spectrum. One audio writer, any number of readers. A load may overlap the next hop, but
/// never waits.
pub struct SharedWave {
    columns: [[AtomicU32; 4]; WAVE_COLUMNS],
    // twice the scope's width: room to start it where the wave rises through zero
    samples: [AtomicU32; 2 * WAVE_COLUMNS],
    spectrum: [AtomicU32; WAVE_COLUMNS],
    written: AtomicUsize,
    sampled: AtomicUsize,
}

impl Default for SharedWave {
    fn default() -> Self {
        Self {
            columns: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            samples: std::array::from_fn(|_| AtomicU32::new(0)),
            spectrum: std::array::from_fn(|_| AtomicU32::new(0)),
            written: AtomicUsize::new(0),
            sampled: AtomicUsize::new(0),
        }
    }
}

impl SharedWave {
    pub fn push_column(&self, values: [f32; 4]) {
        let at = self.written.load(Relaxed);
        for (slot, value) in self.columns[at % WAVE_COLUMNS].iter().zip(values) {
            slot.store(value.to_bits(), Relaxed);
        }
        self.written.store(at.wrapping_add(1), Release);
    }

    pub fn push_samples(&self, samples: &[f32]) {
        let (at, ring) = (self.sampled.load(Relaxed), self.samples.len());
        // Only the last window survives a large capture chunk.
        for (i, value) in samples.iter().enumerate().skip(samples.len().saturating_sub(ring)) {
            self.samples[at.wrapping_add(i) % ring].store(value.to_bits(), Relaxed);
        }
        self.sampled.store(at.wrapping_add(samples.len()), Release);
    }

    /// The latest spectrum (`power` per bin of an FFT `2 × (bins − 1)` points long, at `rate`) as
    /// bands from 30 Hz to 16 kHz evenly in octaves, 0..1: each band's loudest bin (two bins'
    /// blend where a band is narrower than a bin) in dB, tilted up 4.5 dB an octave as music falls
    /// off, rising at once and falling slowly, so bars dance rather than flicker.
    pub fn push_spectrum(&self, power: &[f32], rate: f32) {
        let last = power.len() - 1;
        let bin = |hz: f32| hz * 2.0 * last as f32 / rate;
        let at = |k: f32| {
            let i = (k.floor() as usize).min(last);
            power[i] + (power[(i + 1).min(last)] - power[i]) * k.fract()
        };
        let octaves = (HIGHEST_HZ / LOWEST_HZ).log2();
        let hz = |x: f32| LOWEST_HZ * 2f32.powf(octaves * x / WAVE_COLUMNS as f32);
        for (b, slot) in self.spectrum.iter().enumerate() {
            let (lo, hi) = (bin(hz(b as f32)), bin(hz(b as f32 + 1.0)));
            let p = if hi - lo < 1.0 { at((lo + hi) * 0.5) } else { power[(lo.ceil() as usize).min(last)..=(hi.floor() as usize).min(last)].iter().copied().fold(0.0, f32::max) };
            let tilt = 4.5 * (hz(b as f32 + 0.5) / 1000.0).log2();
            let v = ((10.0 * p.max(1e-12).log10() + tilt + 48.0) / 60.0).clamp(0.0, 1.0);
            let was = f32::from_bits(slot.load(Relaxed));
            let now = if v > was { was + (v - was) * 0.6 } else { was + (v - was) * 0.12 };
            slot.store(now.to_bits(), Relaxed);
        }
    }

    /// Texture rows, oldest first: RGBA bands, then raw mono in R, then the spectrum in R, low
    /// to high. Unfilled history is zero. The scope row starts where the wave last rose through
    /// zero, so a steady tone stands still.
    pub fn load(&self, pixels: &mut [[f32; 4]; WAVE_COLUMNS * WAVE_ROWS]) {
        let column = self.written.load(Acquire) % WAVE_COLUMNS;
        let ring = self.samples.len();
        let sample = self.sampled.load(Acquire) % ring;
        let s = |i: usize| f32::from_bits(self.samples[(sample + i) % ring].load(Relaxed));
        let start = (1..=ring - WAVE_COLUMNS).rev().find(|&i| s(i - 1) <= 0.0 && s(i) > 0.0).unwrap_or(ring - WAVE_COLUMNS);
        for i in 0..WAVE_COLUMNS {
            pixels[i] = self.columns[(column + i) % WAVE_COLUMNS].each_ref().map(|v| f32::from_bits(v.load(Relaxed)));
            pixels[WAVE_COLUMNS + i] = [s(start + i), 0.0, 0.0, 0.0];
            pixels[2 * WAVE_COLUMNS + i] = [f32::from_bits(self.spectrum[i].load(Relaxed)), 0.0, 0.0, 0.0];
        }
    }
}

/// A graph ready to install: compiled (on any thread, with [`Engine::device`]) and allocated.
pub struct Graph {
    pub programs: Vec<Program>,
    pub runner: Runner,
    /// The node whose picture goes out, if any.
    pub output: Option<usize>,
    /// Each node's values and mappings (by node index; a graph without them runs on its
    /// parameter buffers as set).
    pub mods: Vec<Modulator>,
    /// The scene's macros: their values, a morph, what they drive.
    pub macros: Macros,
    /// Its signal nodes, whose outputs follow the audio and clock sources.
    pub signals: Vec<crate::signal::Signal>,
    /// Its Probe nodes' picture sides.
    pub probes: Vec<crate::gpu::Probe>,
    /// Its Image nodes' files, each frame uploaded as it comes due.
    pub media: Vec<crate::media::Playing>,
    /// Its Media Lists, each streaming its files through its slots.
    pub lists: Vec<crate::media::Reel>,
    /// Its nodes that read only some of their inputs (Switches, Blends), by node index.
    pub lazy: Vec<(usize, crate::gpu::Lazy)>,
}

/// UI → render thread. Fire and forget. Graphs go by the key the UI installs them with (one
/// per scene graph it keeps; 0 is none).
pub enum Command {
    /// A scene's graph, new or rebuilt: a rebuilt live graph goes out from the next frame.
    Install {
        key: u64,
        graph: Box<Graph>,
    },
    /// A graph no longer kept warm (the UI never evicts the live one, a launch's or the edited one).
    Evict(u64),
    /// A node's base values (one vec4 per parameter, in header order).
    Params {
        key: u64,
        node: usize,
        values: Vec<[f32; 4]>,
    },
    /// A node's mappings, all of them ([`crate::routes`]).
    Mappings {
        key: u64,
        node: usize,
        routes: Vec<Route>,
    },
    /// A macro's value (0..1), at once: a knob, a controller.
    Macro {
        key: u64,
        index: usize,
        value: f32,
    },
    /// All eight macros glide to new values over some beats of the clock (a variation's morph).
    Morph {
        key: u64,
        to: [f32; MACROS],
        beats: f32,
    },
    /// What the macros drive, all of it ([`crate::macro_routes`]).
    MacroTargets {
        key: u64,
        routes: Vec<MacroRoute>,
    },
    /// Puts a scene's graph on the output (docs/scenes.md §3): once it's installed, at the next
    /// `quantize` boundary of the beat clock, cut or crossfaded over `duration_beats`.
    Launch {
        scene: u64,
        transition: Transition,
        duration_beats: f32,
        quantize: Quantize,
    },
    /// The graph being edited: its node is the watched one, and the preview shows it alone
    /// (rendering it too) while it isn't the live one.
    Edit(u64),
    /// A graph's picture for its scene's card: rendered for [`PEEK`] frames (alone, if it isn't
    /// rendering already), then what it shows goes to [`Engine::take_shot`]. One at a time.
    Peek(u64),
    /// The layer scenes switched on, in scene order: each rendered every frame and put over or
    /// under the program the way its blend says.
    Layers(Vec<(u64, Layer, LayerBlend)>),
    /// Tap tempo, a new tempo, a downbeat now: the beat clock (docs/control.md §4). Taps carry
    /// when they happened, so the time the command waits in the queue doesn't count.
    Tap(Instant),
    Tempo(f32),
    Resync(Instant),
    /// An outside clock's beat (MIDI clock): its tempo, and which beat since its Start it was
    /// (None: joined while running, only the beat's phase is known), at the time it came.
    Follow {
        bpm: f32,
        beat: Option<f64>,
        at: Instant,
    },
    /// A graph's LFOs changed (a shape, a length): their new settings, in the graph's order.
    Signals {
        key: u64,
        kinds: Vec<crate::signal::Kind>,
    },
    Attach {
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
        period: Duration,
    },
    Resize([u32; 2]),
    Detach,
    /// The output goes black (the graph and the preview keep running, so lifting it is seamless).
    Blackout(bool),
    /// The program held on the frame it shows now (outputs, shares and the preview), or let go.
    Freeze(bool),
    /// The outputs and shares show a test pattern instead (lining up a projector), or not.
    Pattern(bool),
    /// The output window is covered or off-screen: don't wait for it to take frames.
    Occluded(bool),
    /// The program shared with other apps (Spout) or over the network (NDI) too, or no longer.
    Share(Option<crate::share::Share>),
    Quit,
}

/// What the UI can read without asking.
#[derive(Default)]
pub struct Status {
    pub frames: AtomicU64,
    /// Output frames that came later than 1.5 refresh periods.
    pub late: AtomicU64,
    /// Times the output didn't hand out a frame in time (a covered or stuck display): the engine
    /// went on without it.
    pub stalls: AtomicU64,
    /// Last frame's CPU time on the render thread, in µs.
    pub frame_us: AtomicU64,
    /// The beat clock, as f32 bits: bpm, the beat's hit, the ramp through the bar.
    pub clock: [AtomicU32; 3],
    /// Beats since the clock's beat 0, as f64 bits (the UI draws LFOs on it).
    pub beats: AtomicU64,
    /// The node whose modulated values the UI shows: its index + 1 (0: none), as the UI asks…
    pub watch: AtomicUsize,
    /// …and as the engine last published them, with the values: x and y of the first 16
    /// parameters (what a slider can map), as f32 bits.
    pub watched: AtomicUsize,
    pub values: [AtomicU32; 32],
    /// The edited graph's signal outputs, by slot (signal::plan's order), as f32 bits.
    pub signals: Signals,
    /// What goes out, by graph key (0: none): the live graph, the one fading out under it,
    /// the launch waiting for its boundary and how far along the wait is (f32 bits).
    pub live: AtomicU64,
    pub fading: AtomicU64,
    pub armed: AtomicU64,
    pub countdown: AtomicU32,
    /// The GPU's last complaint no one caught (it's logged, never a panic), for the UI to say.
    pub gpu_error: Mutex<Option<String>>,
    /// The GPU device is gone (a driver reset, a shader past the system's time limit): nothing
    /// renders until the engine starts again.
    pub lost: std::sync::atomic::AtomicBool,
    /// What each node of the edited graph cost the GPU lately, in ms (f32 bits), by its index in
    /// the graph; the graph's key they're for (0: none yet, or no timestamps on this GPU).
    pub costs: Costs,
    pub costed: AtomicU64,
    /// The edited graph's nodes nothing shows now (a Switch's other inputs…), one bit each for
    /// the first 64: they keep their last picture.
    pub asleep: AtomicU64,
    /// Why sharing stopped (no NDI runtime, no DirectX 11), for the UI to say.
    pub share_error: Mutex<Option<String>>,
}

/// Node costs, published (arrays past 32 have no Default).
pub struct Costs(pub [AtomicU32; crate::gpu::MAX_TIMED]);

impl Default for Costs {
    fn default() -> Self {
        Self(std::array::from_fn(|_| AtomicU32::new(0)))
    }
}

/// A graph's signal outputs, published (arrays past 32 have no Default).
pub struct Signals(pub [AtomicU32; crate::signal::MAX_SIGNALS]);

impl Default for Signals {
    fn default() -> Self {
        Self(std::array::from_fn(|_| AtomicU32::new(0)))
    }
}

impl Status {
    /// The edited graph's signal output in `slot` (signal::plan's order).
    pub fn signal(&self, slot: usize) -> f32 {
        self.signals.0.get(slot).map_or(0.0, |v| f32::from_bits(v.load(Relaxed)))
    }

    /// bpm, the beat's hit (0..1), the ramp through the bar (0..1).
    pub fn clock(&self) -> [f32; 3] {
        self.clock.each_ref().map(|v| f32::from_bits(v.load(Relaxed)))
    }

    /// Shows `node`'s modulated values from the next frame on; returns them if the engine has
    /// published them for it (x and y per parameter, only while the node is mapped).
    pub fn watch(&self, node: usize) -> Option<[f32; 32]> {
        self.watch.store(node + 1, Relaxed);
        (self.watched.load(Relaxed) == node + 1).then(|| self.values.each_ref().map(|v| f32::from_bits(v.load(Relaxed))))
    }

    /// The launch waiting for its boundary, by graph key, and how far along the wait is (0..1).
    pub fn armed(&self) -> Option<(u64, f32)> {
        let key = self.armed.load(Relaxed);
        (key != 0).then(|| (key, f32::from_bits(self.countdown.load(Relaxed))))
    }
}

/// The engine, from the UI's side.
pub struct Engine {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    commands: mpsc::Sender<Command>,
    /// Detached outputs come back here, so their windows are dropped on the main thread.
    pub detached: mpsc::Receiver<()>,
    pub status: Arc<Status>,
    pub features: Arc<SharedFeatures>,
    pub wave: Arc<SharedWave>,
    wave_texture: wgpu::TextureView,
    preview: Arc<Preview>,
    thumbs: Arc<Preview>,
    shots: Arc<Preview>,
}

impl Engine {
    /// Opens the GPU and starts the render thread. `display` is the event loop's display handle
    /// (needed on Wayland/GL; harmless elsewhere).
    pub fn start(display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>) -> Result<Engine, String> {
        let descriptor = match display {
            Some(d) => wgpu::InstanceDescriptor::new_with_display_handle_from_env(d),
            None => wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        };
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).map_err(|e| e.to_string())?;
        // BC textures where the GPU has them (every desktop one): HAP movies' frames
        let features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
        // and timestamps where it has them: what each node costs
        let features = features | (adapter.features() & wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_features: features, ..Default::default() })).map_err(|e| e.to_string())?;
        let (commands, rx) = mpsc::channel();
        let (detached_tx, detached) = mpsc::channel();
        let status = Arc::new(Status::default());
        // wgpu's default panics on any error nobody caught: here it's logged and told the UI
        let told = status.clone();
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let text = crate::gpu_error(&e);
            eprintln!("velo: GPU error: {text}");
            if let Ok(mut last) = told.gpu_error.lock() {
                *last = Some(text);
            }
        }));
        let told = status.clone();
        device.set_device_lost_callback(move |reason, message| {
            eprintln!("velo: GPU device lost ({reason:?}): {message}");
            told.lost.store(true, Relaxed);
        });
        let features = Arc::new(SharedFeatures::default());
        let wave = Arc::new(SharedWave::default());
        let wave_texture = crate::gpu::wave_texture(&device);
        let preview = Arc::new(Preview::new(&device, PREVIEW, PREVIEW_FORMAT));
        let thumbs = Arc::new(Preview::new(&device, [THUMB[0] * THUMBS[0], THUMB[1] * THUMBS[1]], PREVIEW_FORMAT));
        let shots = Arc::new(Preview::new(&device, THUMB, PREVIEW_FORMAT));
        let thread = Thread {
            device: device.clone(),
            queue: queue.clone(),
            commands: rx,
            detached: detached_tx,
            status: status.clone(),
            features: features.clone(),
            wave: wave.clone(),
            wave_texture: wave_texture.clone(),
            preview: preview.clone(),
            thumbs: thumbs.clone(),
            shots: shots.clone(),
        };
        std::thread::Builder::new().name("velo render".into()).spawn(move || thread.run()).map_err(|e| e.to_string())?;
        Ok(Engine { instance, adapter, device, queue, commands, detached, status, features, wave, wave_texture, preview, thumbs, shots })
    }

    /// The engine's device, for compiling programs and building graphs on a worker.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Shared by every graph's audio bindings.
    pub fn wave_texture(&self) -> &wgpu::TextureView {
        &self.wave_texture
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Hands the newest preview frame (RGBA8, sRGB, [`PREVIEW`] sized) to `show`, if one is ready.
    /// The copy happens here, on the caller's thread, never on the render thread.
    pub fn take_preview(&self, show: impl FnOnce(&[u8])) -> bool {
        self.preview.take(show)
    }

    /// Hands the newest pictures of the edited graph's nodes to `show`: RGBA8, sRGB, a grid of
    /// [`THUMBS`] cells of [`THUMB`] each, node n in cell n (left to right, then down).
    pub fn take_thumbs(&self, show: impl FnOnce(&[u8])) -> bool {
        self.thumbs.take(show)
    }

    /// Hands the picture a [`Command::Peek`] asked for to `show`, once it's there: RGBA8, sRGB,
    /// [`THUMB`] sized.
    pub fn take_shot(&self, show: impl FnOnce(&[u8])) -> bool {
        self.shots.take(show)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Quit);
    }
}

// ---------------------------------------------------------------------------------------------

const FREE: u8 = 0;
const PENDING: u8 = 1;
const MAPPED: u8 = 2;

/// A small picture of the output for the UI: drawn small, copied to mappable buffers; the UI
/// copies a mapped one out and frees it.
struct Preview {
    size: [u32; 2],
    /// Bytes a row in its buffers: the width's, padded to what a copy takes (256).
    stride: u32,
    texture: wgpu::Texture,
    staging: [(wgpu::Buffer, Arc<AtomicU8>); 3],
    /// Which buffer the newest mapped picture is in.
    newest: Mutex<Option<usize>>,
}

impl Preview {
    fn new(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (size[0] * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let bytes = u64::from(stride * size[1]);
        let staging = std::array::from_fn(|_| {
            (device.create_buffer(&wgpu::BufferDescriptor { label: Some("preview readback"), size: bytes, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }), Arc::new(AtomicU8::new(FREE)))
        });
        Self { size, stride, texture, staging, newest: Mutex::new(None) }
    }

    /// A free buffer to copy the next picture into, if one is.
    fn free(&self) -> Option<usize> {
        self.staging.iter().position(|(_, s)| s.load(Relaxed) == FREE)
    }

    /// Records the copy of the drawn picture into buffer `i`.
    fn copy(&self, encoder: &mut wgpu::CommandEncoder, i: usize) {
        let [width, height] = self.size;
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.stride), rows_per_image: None };
        encoder.copy_texture_to_buffer(self.texture.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &self.staging[i].0, layout }, wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
    }

    /// After the submit: buffer `i` maps, then it's the newest (an older one untaken goes back).
    fn mapped(self: &Arc<Self>, i: usize) {
        let (buffer, state) = &self.staging[i];
        state.store(PENDING, Relaxed);
        let (state, preview) = (state.clone(), self.clone());
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                state.store(MAPPED, Relaxed);
                match preview.newest.try_lock() {
                    Ok(mut newest) => {
                        if let Some(old) = newest.replace(i) {
                            let (buffer, state) = &preview.staging[old];
                            buffer.unmap();
                            state.store(FREE, Relaxed);
                        }
                    }
                    // the UI is taking one right now: this one goes back, never left mapped
                    Err(_) => {
                        preview.staging[i].0.unmap();
                        state.store(FREE, Relaxed);
                    }
                }
            } else {
                state.store(FREE, Relaxed);
            }
        });
    }

    fn take(&self, show: impl FnOnce(&[u8])) -> bool {
        // ponytail: a Mutex the render thread only try_locks; a lock-free slot if it ever shows in profiles
        let Some(i) = self.newest.lock().ok().and_then(|mut n| n.take()) else { return false };
        let (buffer, state) = &self.staging[i];
        if let Ok(view) = buffer.slice(..).get_mapped_range() {
            show(&view);
        }
        buffer.unmap();
        state.store(FREE, Relaxed);
        true
    }
}

/// What a shared program is read back in: what Spout and NDI take.
const SHARE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// The program read back full size for a sender's thread.
struct Sharing {
    preview: Arc<Preview>,
    view: wgpu::TextureView,
    blit: Blit,
    /// A new picture size for the sender's thread, and its stop.
    swap: mpsc::Sender<Arc<Preview>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Sharing {
    fn start(device: &wgpu::Device, kind: crate::share::Share, size: [u32; 2], status: Arc<Status>) -> Self {
        let preview = Arc::new(Preview::new(device, size, SHARE_FORMAT));
        let view = preview.texture.create_view(&Default::default());
        let (swap, swaps) = mpsc::channel();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (first, quit) = (preview.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("velo share".into()).spawn(move || send_shared(kind, first, &swaps, &quit, &status));
        Self { preview, view, blit: Blit::new(device, SHARE_FORMAT), swap, stop }
    }
}

impl Drop for Sharing {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

/// The sender's thread: each picture read back goes out as it comes; a failure is told and ends it.
fn send_shared(kind: crate::share::Share, mut preview: Arc<Preview>, swaps: &mpsc::Receiver<Arc<Preview>>, stop: &std::sync::atomic::AtomicBool, status: &Status) {
    let fail = |e: String| {
        if let Ok(mut last) = status.share_error.lock() {
            *last = Some(e);
        }
    };
    let mut sender = match crate::share::open(kind, "Velo") {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    while !stop.load(Relaxed) {
        if let Some(p) = swaps.try_iter().last() {
            preview = p;
        }
        let (size, stride) = (preview.size, preview.stride);
        let mut failed = None;
        if !preview.take(|bytes| failed = sender.send(bytes, size, stride).err()) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if let Some(e) = failed {
            return fail(e);
        }
    }
}

/// How often the edited graph's nodes are timed.
const TIMING_EVERY: Duration = Duration::from_millis(250);

/// The edited graph's nodes timed on the GPU: a timestamp before and after each one's pass,
/// resolved and read back like the preview, published in ms.
struct Timing {
    key: u64,
    nodes: usize,
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

impl Timing {
    fn new(device: &wgpu::Device, key: u64, nodes: usize) -> Self {
        let nodes = nodes.clamp(1, crate::gpu::MAX_TIMED);
        let count = 2 * nodes as u32;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("node costs"), ty: wgpu::QueryType::Timestamp, count });
        let bytes = u64::from(count) * 8;
        let buffer = |label, usage| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: bytes, usage, mapped_at_creation: false });
        let resolve = buffer("node costs", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
        let readback = buffer("node costs readback", wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
        Self { key, nodes, set, resolve, readback, state: Arc::new(AtomicU8::new(FREE)) }
    }

    fn copy(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.set, 0..2 * self.nodes as u32, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, self.resolve.size());
    }

    /// After the submit: the timestamps read back, each node's cost published (`period`: ns a tick).
    fn read(&self, status: Arc<Status>, period: f32) {
        self.state.store(PENDING, Relaxed);
        let (buffer, state, key, nodes) = (self.readback.clone(), self.state.clone(), self.key, self.nodes);
        self.readback.map_async(wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                if let Ok(view) = buffer.slice(..).get_mapped_range() {
                    let at = |k: usize| view.get(k * 8..k * 8 + 8).and_then(|b| <[u8; 8]>::try_from(b).ok()).map_or(0, u64::from_le_bytes);
                    let spans: Vec<(u64, u64)> = (0..nodes).map(|n| (at(2 * n), at(2 * n + 1))).collect();
                    let scale = shared(&spans, (1e8 / period.max(1e-6)) as u64);
                    for (slot, (from, to)) in status.costs.0.iter().zip(&spans) {
                        let ms = to.saturating_sub(*from) as f32 * period / 1e6 * scale;
                        slot.store(ms.to_bits(), Relaxed);
                    }
                }
                buffer.unmap();
                status.costed.store(key, Relaxed);
            }
            state.store(FREE, Relaxed);
        });
    }
}

/// What to scale the nodes' spans by so they add up to the time the GPU was busy with them: it
/// runs passes side by side (Apple's do), so the spans overlap and add up to several frames.
/// `spans` are each node's first tick and last; one that ended more than `stale` ticks before
/// the latest wasn't drawn this frame (asleep) and isn't counted. 1 when nothing overlaps.
fn shared(spans: &[(u64, u64)], stale: u64) -> f32 {
    let latest = spans.iter().map(|s| s.1).max().unwrap_or(0);
    let mut fresh: Vec<(u64, u64)> = spans.iter().copied().filter(|&(from, to)| to > from && to + stale >= latest).collect();
    fresh.sort_unstable();
    let sum: u64 = fresh.iter().map(|(from, to)| to - from).sum();
    // the busy time: the spans put end to end where they overlap
    let (mut busy, mut end) = (0u64, 0u64);
    for (from, to) in fresh {
        busy += to.saturating_sub(from.max(end));
        end = end.max(to);
    }
    if sum > busy && busy > 0 { busy as f32 / sum as f32 } else { 1.0 }
}

struct Output {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    blit: Blit,
    period: Duration,
    occluded: bool,
    /// After a frame it didn't hand out, the next time to ask: until then the engine runs on its
    /// own timer, so the preview never waits for a display that isn't taking frames.
    retry_at: Instant,
}

/// How long to leave an output alone after it didn't hand out a frame.
const RETRY: Duration = Duration::from_millis(250);
/// Frames on the GPU at once: the render thread waits for the one before last before it sends
/// another, so a scene heavier than the GPU slows down instead of piling frames (and their
/// buffers) up in memory.
const IN_FLIGHT: usize = 2;
/// The longest the render thread waits for one: past it, it counts a stall and goes on.
const GPU_WAIT: Duration = Duration::from_millis(250);

/// The edited scene while it isn't going out: it renders for its preview and its cards only.
/// Every frame while the frames keep to their budget; once one doesn't, the show comes first:
/// it gets a frame at a time, about a tenth of the time.
#[derive(Default)]
struct Aside {
    /// When its last frame went.
    at: Option<Instant>,
    /// The longest frame since, and how many: a frame's cost shows up to [`IN_FLIGHT`] frames later.
    peak: Duration,
    since: usize,
    slow: bool,
    /// What its skipped frames took, in seconds: its next frame steps by all of it.
    owed: f32,
}

impl Aside {
    fn due(&self, now: Instant) -> bool {
        !self.slow || (self.since > IN_FLIGHT && self.at.is_none_or(|at| now >= at + self.peak * 10))
    }

    fn rendered(&mut self, now: Instant) {
        (self.at, self.peak, self.since) = (Some(now), Duration::ZERO, 0);
    }

    /// After every frame, with what it took of the render thread.
    fn frame(&mut self, work: Duration, budget: Duration) {
        self.since += 1;
        self.peak = self.peak.max(work);
        if self.peak > budget {
            self.slow = true;
        } else if self.since > IN_FLIGHT {
            self.slow = false;
        }
    }
}

struct Thread {
    device: wgpu::Device,
    queue: wgpu::Queue,
    commands: mpsc::Receiver<Command>,
    detached: mpsc::Sender<()>,
    status: Arc<Status>,
    features: Arc<SharedFeatures>,
    wave: Arc<SharedWave>,
    wave_texture: wgpu::TextureView,
    preview: Arc<Preview>,
    thumbs: Arc<Preview>,
    shots: Arc<Preview>,
}

impl Thread {
    fn run(self) {
        let preview_blit = Blit::new(&self.device, PREVIEW_FORMAT);
        let preview_view = self.preview.texture.create_view(&Default::default());
        let thumb_blit = Blit::new(&self.device, PREVIEW_FORMAT);
        let thumbs_view = self.thumbs.texture.create_view(&Default::default());
        let shot_view = self.shots.texture.create_view(&Default::default());
        // the graph rendering for its card's picture, and the frames it has left
        let mut peek: Option<(u64, u32)> = None;
        // what a crossfade shows for a graph with nothing to show
        let black = crate::gpu::texture(&self.device, [1, 1]);
        // Freeze: the program copied once, shown until it's let go
        let held_blit = Blit::new(&self.device, crate::gpu::FORMAT);
        // the layers: the program drawn on a stage, then each layer onto the other side of it
        let (stage_blit, compose) = (Blit::new(&self.device, crate::gpu::FORMAT), Compose::new(&self.device));
        let mut stage: Option<([u32; 2], [wgpu::TextureView; 2])> = None;
        let mut layers: Vec<(u64, Layer, LayerBlend)> = Vec::new();
        let (mut freeze, mut held, mut pattern) = (false, None, false);
        let start = Instant::now();
        let tick = Duration::from_micros(16_667);
        // when the next tick is due, without an output's vsync to pace the frames
        let mut deadline = Instant::now();
        // the scene graphs it keeps (a dozen at most: the UI evicts), the live one and the launches
        let mut graphs: Vec<(u64, Box<Graph>)> = Vec::with_capacity(16);
        let (mut show, mut edit) = (Show::default(), 0u64);
        let mut output: Option<Output> = None;
        let mut blackout = false;
        let mut clock = Clock::default();
        let mut wave = [[0.0f32; 4]; WAVE_COLUMNS * WAVE_ROWS];
        let mut wave_bytes = [0u8; WAVE_COLUMNS * WAVE_ROWS * 16];
        let secs = |at: Instant| at.saturating_duration_since(start).as_secs_f64();
        let (mut last, mut last_preview, mut last_thumbs, mut frame) = (start, start, start, 0u64);
        let mut in_flight = std::collections::VecDeque::with_capacity(IN_FLIGHT + 1);
        // the edited graph's nodes timed, where the GPU has timestamps
        let timed = self.device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let period = self.queue.get_timestamp_period();
        let (mut timing, mut last_timing): (Option<Timing>, Instant) = (None, start);
        let mut sharing: Option<Sharing> = None;
        let mut aside = Aside::default();
        loop {
            for command in self.commands.try_iter() {
                match command {
                    Command::Install { key, mut graph } => match graphs.iter_mut().find(|g| g.0 == key) {
                        Some(slot) => {
                            // a Media List's files in (or loading) carry over: a rebuild doesn't show them black
                            for new in &mut graph.lists {
                                for old in &mut slot.1.lists {
                                    new.adopt(old);
                                }
                            }
                            drop_later(std::mem::replace(&mut slot.1, graph))
                        }
                        None => graphs.push((key, graph)),
                    },
                    Command::Evict(key) => {
                        if let Some(i) = graphs.iter().position(|g| g.0 == key) {
                            drop_later(graphs.swap_remove(i).1);
                        }
                    }
                    Command::Params { key, node, values } => {
                        if let Some(g) = find(&mut graphs, key)
                            && node < g.runner.len()
                        {
                            match g.mods.get_mut(node) {
                                // a mapped node is written every frame below
                                Some(m) => {
                                    m.set_base(&values);
                                    if !m.active() {
                                        g.runner.write_params(&self.queue, node, m.base_bytes());
                                    }
                                }
                                None => g.runner.set_params(&self.queue, &g.programs, node, &values),
                            }
                        }
                    }
                    Command::Mappings { key, node, routes } => {
                        if let Some(g) = find(&mut graphs, key)
                            && let Some(m) = g.mods.get_mut(node)
                        {
                            m.set_routes(routes);
                            // nothing mapped any more: back to the base values
                            if !m.active() {
                                g.runner.write_params(&self.queue, node, m.base_bytes());
                            }
                        }
                    }
                    Command::Macro { key, index, value } => {
                        if let Some(g) = find(&mut graphs, key) {
                            g.macros.set(index, value);
                        }
                    }
                    Command::Morph { key, to, beats } => {
                        if let Some(g) = find(&mut graphs, key) {
                            g.macros.morph(clock.beats(secs(Instant::now())), to, beats);
                        }
                    }
                    Command::MacroTargets { key, routes } => {
                        if let Some(g) = find(&mut graphs, key) {
                            // the old list is dropped here: a few bytes, on an edit, not per frame
                            g.macros.routes = routes;
                            g.macros.drive(&mut g.mods);
                            // a node no longer driven: its base values again (the UI resends them next)
                            for (n, m) in g.mods.iter_mut().enumerate().filter(|(_, m)| !m.active()) {
                                g.runner.write_params(&self.queue, n, m.base_bytes());
                            }
                        }
                    }
                    Command::Launch { scene, transition, duration_beats, quantize } => show.launch(scene, transition, duration_beats, quantize),
                    Command::Edit(key) => edit = key,
                    Command::Peek(key) => peek = Some((key, PEEK)),
                    Command::Layers(on) => layers = on,
                    Command::Tap(at) => clock.tap(secs(at)),
                    Command::Tempo(bpm) => clock.set_bpm(start.elapsed().as_secs_f64(), bpm),
                    Command::Resync(at) => clock.resync(secs(at)),
                    Command::Follow { bpm, beat, at } => clock.follow(secs(at), bpm, beat),
                    Command::Signals { key, kinds } => {
                        if let Some((_, g)) = graphs.iter_mut().find(|(k, _)| *k == key) {
                            crate::signal::retune(&mut g.signals, &kinds);
                        }
                    }
                    Command::Attach { surface, config, period } => {
                        surface.configure(&self.device, &config);
                        output = Some(Output { blit: Blit::new(&self.device, config.format), surface, config, period, occluded: false, retry_at: Instant::now() });
                    }
                    Command::Resize([w, h]) => {
                        if let Some(o) = &mut output
                            && w > 0
                            && h > 0
                        {
                            // a window dragged wider than the GPU's largest texture stays as large as it takes
                            let max = self.device.limits().max_texture_dimension_2d;
                            (o.config.width, o.config.height) = (w.min(max), h.min(max));
                            o.surface.configure(&self.device, &o.config);
                        }
                    }
                    Command::Detach => {
                        output = None;
                        let _ = self.detached.send(());
                    }
                    Command::Blackout(on) => blackout = on,
                    Command::Freeze(on) => {
                        freeze = on;
                        held = None;
                    }
                    Command::Pattern(on) => pattern = on,
                    Command::Occluded(hidden) => {
                        if let Some(o) = &mut output {
                            o.occluded = hidden;
                        }
                    }
                    Command::Share(kind) => {
                        let size = graphs.iter().find(|g| g.0 == show.live).map_or([1920, 1080], |(_, g)| g.runner.size());
                        sharing = kind.map(|k| Sharing::start(&self.device, k, size, self.status.clone()));
                    }
                    Command::Quit => return,
                }
            }
            // pace: the output's vsync when it takes frames, else a 60 Hz tick. A display that
            // doesn't (covered, off-screen, stuck) never stops the graph or the preview (ARCHITECTURE.md §5)
            let target = match &mut output {
                Some(o) if !o.occluded && Instant::now() >= o.retry_at => match o.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        o.surface.configure(&self.device, &o.config);
                        None
                    }
                    _ => {
                        // ponytail: one acquire can still block (Metal waits up to 1 s for a
                        // drawable); a present thread per output would hide even that
                        o.retry_at = Instant::now() + RETRY;
                        self.status.stalls.fetch_add(1, Relaxed);
                        None
                    }
                },
                _ => None,
            };
            if target.is_none() {
                // to the next tick on a fixed beat, not a tick after waking: a wake that comes
                // late is made up on the next frame instead of slowing every one (a deadline
                // missed by more than a tick starts the beat again from now)
                let now = Instant::now();
                deadline = if deadline + tick < now { now } else { deadline + tick };
                std::thread::sleep(deadline.saturating_duration_since(now));
            }
            let now = Instant::now();
            if let (Some(o), Some(_)) = (&output, &target)
                && now - last > o.period * 3 / 2
            {
                self.status.late.fetch_add(1, Relaxed);
            }
            let delta = (now - last).as_secs_f32();
            last = now;
            // what a frame may take: the output's period while it paces the frames, else the tick
            let budget = match (&output, &target) {
                (Some(o), Some(_)) => o.period,
                _ => tick,
            };

            // launches: a waiting one lands on its boundary once its graph is built
            let t = secs(now);
            show.step(t, clock.beats(t), clock.bpm(), |key| graphs.iter().any(|g| g.0 == key));
            let fade = show.fade(t);
            let fading = fade.map_or(0, |f| f.0);
            let armed = show.armed(clock.beats(t));
            for (slot, key) in [(&self.status.live, show.live), (&self.status.fading, fading), (&self.status.armed, armed.map_or(0, |a| a.0))] {
                slot.store(key, Relaxed);
            }
            self.status.countdown.store(armed.map_or(0.0, |a| a.1).to_bits(), Relaxed);

            // sources: the audio features, then the clock (velo_core::SOURCES' order)
            let [beat_phase, bar_phase, beat] = clock.at(t);
            let features = self.features.load();
            let mut sources = [0.0f32; velo_core::SOURCES.len() + crate::signal::MAX_SIGNALS];
            sources[..7].copy_from_slice(&features[..7]);
            sources[7..velo_core::SOURCES.len()].copy_from_slice(&[beat, bar_phase, features[11]]);
            for (slot, v) in self.status.clock.iter().zip([clock.bpm(), beat, bar_phase]) {
                slot.store(v.to_bits(), Relaxed);
            }
            let mut encoder = self.device.create_command_encoder(&Default::default());
            // the edited scene that isn't going out rests while the show needs the frames
            let apart = edit != 0 && edit != show.live && edit != fading && !layers.iter().any(|l| l.0 == edit);
            if !apart {
                aside = Aside::default();
            }
            let resting = apart && !aside.due(now);
            if resting {
                aside.owed += delta;
            } else if apart {
                aside.rendered(now);
            }
            // a graph going out is pictured as it is, at once
            if let Some((key, left)) = &mut peek
                && (*key == show.live || *key == fading)
            {
                *left = 1;
            }
            let peeked = peek.map_or(0, |p| p.0);
            let renders = |key: &u64| !(resting && *key == edit) && ([show.live, fading, edit].contains(key) || layers.iter().any(|l| l.0 == *key)) || (*key == peeked && peeked != 0);
            if graphs.iter().any(|(key, g)| renders(key) && g.programs.iter().any(|p| p.node.audio)) {
                self.wave.load(&mut wave);
                for (bytes, value) in wave_bytes.as_chunks_mut::<4>().0.iter_mut().zip(wave.iter().flatten()) {
                    *bytes = value.to_ne_bytes();
                }
                self.queue.write_texture(
                    self.wave_texture.texture().as_image_copy(),
                    &wave_bytes,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(WAVE_COLUMNS as u32 * 16), rows_per_image: None },
                    wgpu::Extent3d { width: WAVE_COLUMNS as u32, height: WAVE_ROWS as u32, depth_or_array_layers: 1 },
                );
            }
            // the edited graph timed now and then (a set for it as it is: rebuilt, a new one)
            if timed && let Some((_, g)) = graphs.iter().find(|g| g.0 == edit && edit != 0) {
                let nodes = g.runner.len().min(crate::gpu::MAX_TIMED);
                if nodes > 0 && timing.as_ref().is_none_or(|x| x.key != edit || x.nodes != nodes) {
                    timing = Some(Timing::new(&self.device, edit, nodes));
                }
            }
            let time_now = timed && !resting && now - last_timing >= TIMING_EVERY &&timing.as_ref().is_some_and(|x| x.key == edit && x.state.load(Relaxed) == FREE);
            // what renders, each graph once: the program (two graphs in a crossfade) and the
            // edited graph; the warm ones wait
            let watch = self.status.watch.load(Relaxed);
            let beats = clock.beats(t);
            self.status.beats.store(beats.to_bits(), Relaxed);
            for (key, g) in graphs.iter_mut().filter(|(k, _)| renders(k)) {
                // after a rest, the time it rested too: its motion keeps its speed
                let delta = if apart && *key == edit { delta + std::mem::take(&mut aside.owed) } else { delta };
                // this graph's LFOs after the shared sources; its Probes with what they last read
                for p in &g.probes {
                    g.signals[p.signal].feed(p.color());
                }
                crate::signal::run(&mut g.signals, &mut sources, beats, delta);
                if *key == edit {
                    for (slot, v) in self.status.signals.0.iter().zip(&sources[velo_core::SOURCES.len()..]) {
                        slot.store(v.to_bits(), Relaxed);
                    }
                }
                // the macros first: a driven parameter's base, then the mappings on top
                g.macros.apply(beats, &mut g.mods);
                for (n, m) in g.mods.iter_mut().enumerate().filter(|(_, m)| m.active()) {
                    g.runner.write_params(&self.queue, n, m.step(&sources, delta));
                    if *key == edit && watch == n + 1 {
                        for (slot, v) in self.status.values.iter().zip(m.values().iter().flat_map(|v| [v[0], v[1]])) {
                            slot.store(v.to_bits(), Relaxed);
                        }
                        self.status.watched.store(watch, Relaxed);
                    }
                }
                // the files' frames due, at each node's speed, in the part it loops (mappable like
                // any parameter)
                for p in &mut g.media {
                    let value = |i: usize, or: f32| g.mods.get(p.node).map_or(or, |m| m.value(i));
                    let speed = p.speed.map_or(1.0, |i| value(i, 1.0));
                    let window = p.range.map_or((0.0, 1.0), |(a, b)| (value(a, 0.0), value(b, 1.0)));
                    p.step(&self.queue, delta, speed, window);
                }
                let [w, h] = g.runner.size();
                let mut globals = [0.0f32; 20];
                globals[..7].copy_from_slice(&[start.elapsed().as_secs_f32(), delta, frame as f32, w as f32 / h.max(1) as f32, clock.bpm(), beat_phase, bar_phase]);
                globals[7..19].copy_from_slice(&features);
                // the beats counted so far (a Media List steps on them)
                globals[19] = clock.beats(t) as f32;
                // a Media List's turn: its file in, the next loading
                for r in &mut g.lists {
                    let node = r.node;
                    r.step(&self.device, &self.queue, &mut g.runner, &g.programs, |p| g.mods.get(node).map_or(0.0, |m| m.value(p)), globals[19], globals[0], delta);
                }
                let set = timing.as_ref().filter(|x| time_now && *key == edit && x.key == edit).map(|x| &x.set);
                // what nobody sees sleeps: a Switch's other inputs, a Blend's at no opacity, what
                // doesn't reach the output. The edited graph's sleepers wake a frame in eight, for
                // their cards, unless it's going out: a heavy one waking must never drop a frame
                // of the show.
                let awake = g.output.map(|out| {
                    let reads = |n: usize, i: usize| g.lazy.iter().find(|l| l.0 == n).is_none_or(|(_, l)| l.reads(|p| g.mods.get(n).map_or(0.0, |m| m.value(p)), i));
                    let mut awake = g.runner.needed(std::iter::once(out).chain(g.probes.iter().map(|p| p.node)), reads);
                    if *key == edit {
                        self.status.asleep.store(awake.iter().take(64).enumerate().filter(|(_, a)| !**a).fold(0, |bits, (n, _)| bits | 1 << n), Relaxed);
                        if apart {
                            for (n, a) in awake.iter_mut().enumerate() {
                                *a |= (frame + n as u64).is_multiple_of(8);
                            }
                        }
                    }
                    awake
                });
                if *key == edit && awake.is_none() {
                    self.status.asleep.store(0, Relaxed);
                }
                g.runner.render_timed(&self.queue, &mut encoder, &g.programs, &globals, set, awake.as_deref());
                for p in &g.probes {
                    if let Some(crate::signal::Kind::Probe(s)) = g.signals.get(p.signal).map(|s| s.kind) {
                        p.encode(&self.queue, &mut encoder, &g.runner, s.position, s.size);
                    }
                }
            }
            if time_now && let Some(x) = &timing {
                x.copy(&mut encoder);
                last_timing = now;
            }
            let picture = |key: u64| graphs.iter().find(|g| g.0 == key).and_then(|(_, g)| g.output.map(|n| g.runner.output(n)));
            // the program: the live graph, or the crossfade into it (black for a side with
            // nothing to show); the preview shows it while the live graph is the edited one
            let style = Transition::ALL.iter().position(|x| *x == show.transition()).unwrap_or(1) as f32;
            let program = match fade {
                Some((from, p)) => match (picture(from), picture(show.live)) {
                    (None, None) => None,
                    (a, b) => Some((a.unwrap_or(&black), b.unwrap_or(&black), p)),
                },
                None => picture(show.live).map(|v| (v, v, 0.0)),
            };
            let size = graphs.iter().find(|g| g.0 == show.live).map(|(_, g)| g.runner.size());
            // the layers switched on: those under the program, then those over it (one that is
            // the live scene shows once, as the scene)
            let shown: Vec<_> = layers.iter().filter(|l| ![show.live, fading].contains(&l.0)).filter_map(|&(key, layer, blend)| Some((picture(key)?, layer, blend))).collect();
            let size = size.or_else(|| layers.iter().find_map(|l| graphs.iter().find(|g| g.0 == l.0).map(|(_, g)| g.runner.size())));
            let program = match size.filter(|_| !shown.is_empty()) {
                Some(size) => {
                    if stage.as_ref().is_none_or(|s| s.0 != size) {
                        stage = Some((size, [crate::gpu::texture(&self.device, size), crate::gpu::texture(&self.device, size)]));
                    }
                    let views = &stage.as_ref().expect("made above").1;
                    let (a, b, p) = program.unwrap_or((&black, &black, 0.0));
                    stage_blit.draw(&self.device, &self.queue, &mut encoder, [a, b], (p, style), &views[0], None);
                    let mut at = 0;
                    for &(view, layer, blend) in shown.iter().filter(|l| l.1 == Layer::Bottom).chain(shown.iter().filter(|l| l.1 == Layer::Top)) {
                        let (under, over) = if layer == Layer::Top { (&views[at], view) } else { (view, &views[at]) };
                        compose.draw(&self.device, &mut encoder, [under, over], blend, &views[1 - at]);
                        at = 1 - at;
                    }
                    Some((&views[at], &views[at], 0.0))
                }
                None => program,
            };
            if freeze
                && held.is_none()
                && let (Some((a, b, p)), Some(size)) = (program, size)
            {
                let view = crate::gpu::texture(&self.device, size);
                held_blit.draw(&self.device, &self.queue, &mut encoder, [a, b], (p, style), &view, None);
                held = Some(view);
            }
            let program = held.as_ref().map(|v| (v, v, 0.0)).or(program);
            let preview = if edit == 0 || edit == show.live { program } else { picture(edit).map(|v| (v, v, 0.0)) };
            let mut copy_into = None;
            if let Some(t) = &target {
                let view = t.texture.create_view(&Default::default());
                match (program, &output) {
                    (_, Some(o)) if pattern && !blackout => o.blit.draw(&self.device, &self.queue, &mut encoder, [&black, &black], (0.0, PATTERN), &view, None),
                    (Some((a, b, p)), Some(o)) if !blackout => {
                        // its shape kept on a display of another (bars, never stretched)
                        let cell = size.and_then(|s| fit([o.config.width, o.config.height], s));
                        if cell.is_some() {
                            clear(&mut encoder, &view);
                        }
                        o.blit.draw(&self.device, &self.queue, &mut encoder, [a, b], (p, style), &view, cell);
                    }
                    // blackout, or nothing to show: a cleared frame, never whatever the drawable held
                    _ => clear(&mut encoder, &view),
                }
            }
            // shared: the program full size, read back for the sender (black in a blackout), at the
            // live graph's size as it is
            let mut share_into = None;
            if let Some(s) = &mut sharing {
                if let Some(size) = graphs.iter().find(|g| g.0 == show.live).map(|(_, g)| g.runner.size())
                    && size != s.preview.size
                {
                    let fresh = Arc::new(Preview::new(&self.device, size, SHARE_FORMAT));
                    s.view = fresh.texture.create_view(&Default::default());
                    let _ = s.swap.send(fresh.clone());
                    s.preview = fresh;
                }
                share_into = s.preview.free();
                if let Some(i) = share_into {
                    match program {
                        _ if pattern && !blackout => s.blit.draw(&self.device, &self.queue, &mut encoder, [&black, &black], (0.0, PATTERN), &s.view, None),
                        Some((a, b, p)) if !blackout => s.blit.draw(&self.device, &self.queue, &mut encoder, [a, b], (p, style), &s.view, None),
                        _ => clear(&mut encoder, &s.view),
                    }
                    s.preview.copy(&mut encoder, i);
                }
            }
            if let Some((a, b, p)) = preview
                && now - last_preview >= PREVIEW_EVERY
            {
                copy_into = self.preview.free();
                if let Some(i) = copy_into {
                    last_preview = now;
                    preview_blit.draw(&self.device, &self.queue, &mut encoder, [a, b], (p, style), &preview_view, None);
                    self.preview.copy(&mut encoder, i);
                }
            }
            // the edited graph's nodes, each in its cell, for the canvas' cards
            let mut thumbs_into = None;
            if now - last_thumbs >= THUMBS_EVERY
                && let Some((_, g)) = graphs.iter().find(|g| g.0 == edit)
            {
                thumbs_into = self.thumbs.free();
                if let Some(i) = thumbs_into {
                    last_thumbs = now;
                    clear(&mut encoder, &thumbs_view);
                    for n in 0..g.runner.len().min((THUMBS[0] * THUMBS[1]) as usize) {
                        let (col, row) = (n as u32 % THUMBS[0], n as u32 / THUMBS[0]);
                        let cell = [(col * THUMB[0]) as f32, (row * THUMB[1]) as f32, THUMB[0] as f32, THUMB[1] as f32];
                        let picture = g.runner.output(n);
                        thumb_blit.draw(&self.device, &self.queue, &mut encoder, [picture, picture], (0.0, 1.0), &thumbs_view, Some(cell));
                    }
                    self.thumbs.copy(&mut encoder, i);
                }
            }
            // a card's picture, once its graph has rendered long enough (one gone meanwhile: none)
            let mut shot_into = None;
            if let Some((key, left)) = &mut peek {
                *left = left.saturating_sub(1);
                if *left == 0 {
                    shot_into = picture(*key).and_then(|view| Some((view, self.shots.free()?)));
                    if let Some((view, i)) = shot_into {
                        thumb_blit.draw(&self.device, &self.queue, &mut encoder, [view, view], (0.0, 1.0), &shot_view, None);
                        self.shots.copy(&mut encoder, i);
                    }
                    // no buffer free yet (the UI hasn't taken the last): next frame
                    if shot_into.is_some() || picture(*key).is_none() {
                        peek = None;
                    }
                }
            }
            let shot_into = shot_into.map(|s| s.1);
            in_flight.push_back(self.queue.submit([encoder.finish()]));
            for p in graphs.iter().flat_map(|(_, g)| &g.probes) {
                p.read();
            }
            if let (Some(i), Some(s)) = (share_into, &sharing) {
                s.preview.mapped(i);
            }
            if time_now && let Some(x) = &timing {
                x.read(self.status.clone(), period);
            }
            if let Some(t) = target {
                self.queue.present(t);
            }
            // no more than IN_FLIGHT frames queued: the GPU sets the pace once it's the slower
            if in_flight.len() > IN_FLIGHT
                && let Some(oldest) = in_flight.pop_front()
                && self.device.poll(wgpu::PollType::Wait { submission_index: Some(oldest), timeout: Some(GPU_WAIT) }).is_err()
            {
                self.status.stalls.fetch_add(1, Relaxed);
            }
            // a newer picture replaces the one the UI hasn't taken; the old one goes back
            if let Some(i) = copy_into {
                self.preview.mapped(i);
            }
            if let Some(i) = thumbs_into {
                self.thumbs.mapped(i);
            }
            if let Some(i) = shot_into {
                self.shots.mapped(i);
            }
            let _ = self.device.poll(wgpu::PollType::Poll);
            frame += 1;
            self.status.frames.fetch_add(1, Relaxed);
            self.status.frame_us.store(now.elapsed().as_micros() as u64, Relaxed);
            if apart {
                aside.frame(now.elapsed(), budget);
            }
        }
    }
}

fn find(graphs: &mut [(u64, Box<Graph>)], key: u64) -> Option<&mut Graph> {
    graphs.iter_mut().find(|g| g.0 == key).map(|g| &mut *g.1)
}

/// Drops a graph on a worker, not here: freeing GPU memory can take a while.
fn drop_later(graph: Box<Graph>) {
    std::thread::spawn(move || drop(graph));
}

/// Puts a layer and what's under it together, a [`LayerBlend`] each: one pipeline per blend, its
/// constant set when built, so the passes of a frame share no uniform.
struct Compose {
    pipelines: Vec<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl Compose {
    fn new(device: &wgpu::Device) -> Self {
        const SHADER: &str = "
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return V(vec4<f32>(p * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), p);
}
@group(0) @binding(0) var under: texture_2d<f32>;
@group(0) @binding(1) var over: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
// LayerBlend::ALL's index: over (premultiplied), add, screen, multiply
override BLEND: u32 = 0u;
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
    let a = textureSample(under, smp, v.uv);
    let b = textureSample(over, smp, v.uv);
    var c = b.rgb + a.rgb * (1.0 - b.a);
    if BLEND == 1u {
        c = a.rgb + b.rgb;
    } else if BLEND == 2u {
        c = 1.0 - (1.0 - clamp(a.rgb, vec3<f32>(0.0), vec3<f32>(1.0))) * (1.0 - clamp(b.rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    } else if BLEND == 3u {
        // where the upper one is clear it changes nothing
        c = a.rgb * (b.rgb + 1.0 - b.a);
    }
    return vec4<f32>(c, 1.0 - (1.0 - a.a) * (1.0 - b.a));
}";
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("compose"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let texture = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("compose"), entries: &[entry(0, texture), entry(1, texture), entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering))] });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("compose"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipelines = (0..LayerBlend::ALL.len())
            .map(|blend| {
                let constants = [("BLEND", blend as f64)];
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("compose"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fs"),
                        compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
                        targets: &[Some(wgpu::ColorTargetState { format: crate::gpu::FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { pipelines, layout, sampler }
    }

    /// Draws `[under, over]` put together the way `blend` says over all of `dst`.
    fn draw(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, [under, over]: [&wgpu::TextureView; 2], blend: LayerBlend, dst: &wgpu::TextureView) {
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(under) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(over) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("compose"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: dst, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store }, depth_slice: None })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let blend = LayerBlend::ALL.iter().position(|b| *b == blend).unwrap_or(0);
        pass.set_pipeline(&self.pipelines[blend]);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// A render pass that only clears `view` to black.
fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("black"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store }, depth_slice: None })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
}

/// Where a picture of `size` goes in a target of `into` pixels with its shape kept (bars on two
/// sides), or None when the shapes already match.
fn fit(into: [u32; 2], size: [u32; 2]) -> Option<[f32; 4]> {
    let ([iw, ih], [w, h]) = (into.map(|v| v as f32), size.map(|v| v.max(1) as f32));
    let scale = (iw / w).min(ih / h);
    let (fw, fh) = ((w * scale).round(), (h * scale).round());
    (iw - fw >= 1.0 || ih - fh >= 1.0).then(|| [((iw - fw) / 2.0).floor(), ((ih - fh) / 2.0).floor(), fw, fh])
}

/// The blit's style for the test pattern, past every transition's.
const PATTERN: f32 = 10.0;

/// Copies a picture into a target of any format (or into a cell of it), or a crossfade of two
/// pictures: the program's blend is this one pass. Also draws the test pattern.
struct Blit {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// x: 1 when the target is sRGB (our pictures hold display values, so decode before it
    /// encodes); y: how much of the second picture. Written by each draw: a blit draws once a frame.
    uniform: wgpu::Buffer,
    decode: f32,
}

impl Blit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        const SHADER: &str = "
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return V(vec4<f32>(p * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), p);
}
@group(0) @binding(0) var a: texture_2d<f32>;
@group(0) @binding(1) var b: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
@group(0) @binding(3) var<uniform> u: vec4<f32>;
fn hash(p: vec2<f32>) -> f32 { return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453); }
fn finish(c: vec3<f32>) -> vec4<f32> {
    if u.x > 0.5 { return vec4<f32>(pow(max(c, vec3<f32>(0.0)), vec3<f32>(2.2)), 1.0); }
    return vec4<f32>(c, 1.0);
}
// within `w` pixels of a line where `d` is 0 (1 on it, 0 past it)
fn line(d: f32, w: f32) -> f32 { return 1.0 - smoothstep(w - 0.5, w + 0.5, abs(d) / max(fwidth(d), 1e-6)); }
// for lining up a projector: square cells, the middle crossed, a circle that shows any stretch,
// the edge (anything cut off shows), color bars and a gray ramp
fn pattern(uv: vec2<f32>, aspect: f32) -> vec3<f32> {
    let p = (uv - 0.5) * vec2<f32>(aspect, 1.0);
    let cells = p * 9.0;
    var c = vec3<f32>(0.04) + 0.3 * max(line(fract(cells.x + 0.5) - 0.5, 0.5), line(fract(cells.y + 0.5) - 0.5, 0.5));
    var bars = array<vec3<f32>, 8>(vec3<f32>(1.0), vec3<f32>(1.0, 1.0, 0.0), vec3<f32>(0.0, 1.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0));
    if abs(p.x) < 0.36 && p.y > 0.1 && p.y < 0.22 { c = bars[min(u32((p.x + 0.36) / 0.09), 7u)]; }
    if abs(p.x) < 0.36 && p.y > 0.24 && p.y < 0.3 { c = vec3<f32>(floor((p.x + 0.36) / 0.72 * 11.0) / 10.0); }
    let accent = vec3<f32>(0.13, 0.83, 0.93);
    c = mix(c, vec3<f32>(1.0), line(length(p) - 0.45, 1.0));
    c = mix(c, accent, max(line(p.x, 1.0), line(p.y, 1.0)) * step(length(p), 0.08));
    c = mix(c, accent, max(line(min(uv.x, 1.0 - uv.x), 2.0), line(min(uv.y, 1.0 - uv.y), 2.0)));
    return c;
}
// u: decode for an sRGB target, how far from a to b, which transition (Transition::ALL's index,
// or PATTERN), the target's width over its height
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
    if u.z > 9.5 { return finish(pattern(v.uv, u.w)); }
    let t = clamp(u.y, 0.0, 1.0);
    let style = u.z;
    let s = smoothstep(0.0, 1.0, t);
    // where each is read: Zoom pushes the old one in and brings the new one from further away
    var ua = v.uv;
    var ub = v.uv;
    if style > 2.5 && style < 3.5 {
        ua = (v.uv - 0.5) / (1.0 + 1.5 * s) + 0.5;
        ub = (v.uv - 0.5) * (1.0 + 0.6 * (1.0 - s)) + 0.5;
    }
    // Glitch: 16 x 9 blocks turn over at random, colors split most in the middle of it
    let glitch = style > 3.5 && style < 4.5;
    let block = floor(v.uv * vec2<f32>(16.0, 9.0));
    let split = select(0.0, sin(t * 3.14159) * 0.03 * (hash(block + 7.0) - 0.5), glitch);
    let ca = textureSample(a, smp, ua).rgb;
    let cb = textureSample(b, smp, ub).rgb;
    let red = textureSample(b, smp, ub + vec2<f32>(split, 0.0)).r;
    let blue = textureSample(b, smp, ub - vec2<f32>(split, 0.0)).b;
    var c = mix(ca, cb, t);
    if style > 1.5 && style < 2.5 {
        c = mix(ca, cb, smoothstep(v.uv.x - 0.06, v.uv.x + 0.06, t * 1.12 - 0.06));
    } else if style > 2.5 && style < 3.5 {
        c = mix(ca, cb, s);
    } else if glitch {
        c = mix(ca, vec3<f32>(red, cb.g, blue), step(hash(block), t));
    } else if style > 4.5 {
        // Flash: the old one burns out to white, the new one comes up out of it
        let white = pow(1.0 - abs(t * 2.0 - 1.0), 1.5);
        c = mix(select(ca, cb, t >= 0.5), vec3<f32>(1.0), white);
    }
    return finish(c);
}";
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
        let texture = wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                entry(0, texture),
                entry(1, texture),
                entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
                entry(3, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("blit"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("blit"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Self { pipeline, layout, sampler, uniform, decode: if format.is_srgb() { 1.0 } else { 0.0 } }
    }

    /// Draws `src` (the first going over to the second by `mix`, 0..1, the way `style` says:
    /// [`Transition::ALL`]'s index) over all of `dst`, or into one `cell` of it (x, y, width,
    /// height in pixels), leaving the rest as it is.
    #[allow(clippy::too_many_arguments)]
    fn draw(&self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, src: [&wgpu::TextureView; 2], (mix, style): (f32, f32), dst: &wgpu::TextureView, cell: Option<[f32; 4]>) {
        let mut bytes = [0u8; 16];
        bytes[..4].copy_from_slice(&self.decode.to_ne_bytes());
        bytes[4..8].copy_from_slice(&mix.to_ne_bytes());
        bytes[8..12].copy_from_slice(&style.to_ne_bytes());
        let [w, h] = cell.map_or_else(|| [dst.texture().width() as f32, dst.texture().height() as f32], |c| [c[2], c[3]]);
        bytes[12..16].copy_from_slice(&(w / h.max(1.0)).to_ne_bytes());
        queue.write_buffer(&self.uniform, 0, &bytes);
        // ponytail: one small bind group per draw; cache per source view if it ever shows in profiles
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src[0]) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src[1]) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.uniform.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst,
                resolve_target: None,
                ops: wgpu::Operations { load: if cell.is_some() { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) }, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some([x, y, w, h]) = cell {
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BUILTINS, NodeSpec, ShaderNode};

    #[test]
    fn overlapping_node_spans_add_up_to_the_busy_time() {
        // apart: as measured
        assert_eq!(shared(&[(0, 10), (10, 30)], 1000), 1.0);
        // three side by side over the same 10 ticks, and one alone for 10: 40 ticks of spans, 20 busy
        assert_eq!(shared(&[(100, 110), (100, 110), (100, 110), (110, 120)], 1000), 0.5);
        // one asleep, its ticks from long ago, isn't counted
        assert_eq!(shared(&[(5000, 5010), (5000, 5010), (0, 900)], 1000), 0.5);
        assert_eq!(shared(&[], 1000), 1.0);
    }

    #[test]
    fn the_edited_scene_rests_once_a_frame_goes_over_budget() {
        let (ms, t0) = (Duration::from_millis, Instant::now());
        let mut a = Aside::default();
        // frames within the budget: every one
        for _ in 0..10 {
            assert!(a.due(t0));
            a.rendered(t0);
            a.frame(ms(5), ms(16));
        }
        // one of 100 ms: none for a second, then one alone
        a.rendered(t0);
        a.frame(ms(100), ms(16));
        assert!(!a.due(t0 + ms(900)));
        a.frame(ms(5), ms(16));
        a.frame(ms(5), ms(16));
        assert!(!a.due(t0 + ms(900)) && a.due(t0 + ms(1000)));
        // that one cheap, seen once the frames in flight are through: every frame again
        a.rendered(t0 + ms(1000));
        a.frame(ms(5), ms(16));
        assert!(!a.due(t0 + ms(1016)), "not judged yet");
        a.frame(ms(5), ms(16));
        a.frame(ms(5), ms(16));
        assert!(a.due(t0 + ms(1048)));
    }

    #[test]
    fn a_picture_keeps_its_shape_on_any_display() {
        assert_eq!(fit([1920, 1080], [1920, 1080]), None);
        assert_eq!(fit([3840, 2160], [1920, 1080]), None, "the same shape, larger");
        assert_eq!(fit([1024, 768], [1920, 1080]), Some([0.0, 96.0, 1024.0, 576.0]), "bars above and below");
        assert_eq!(fit([1920, 1080], [1080, 1080]), Some([420.0, 0.0, 1080.0, 1080.0]), "bars on the sides");
    }

    #[test]
    fn wave_columns_are_oldest_first_and_wrap() {
        let wave = SharedWave::default();
        let mut pixels = [[1.0; 4]; WAVE_COLUMNS * WAVE_ROWS];
        wave.load(&mut pixels);
        assert!(pixels.iter().all(|p| *p == [0.0; 4]));
        let column = |i: usize| [i as f32 / 2048.0, 0.2, 0.3, 0.4];
        wave.push_column(column(0));
        wave.push_column(column(1));
        wave.load(&mut pixels);
        assert!(pixels[..WAVE_COLUMNS - 2].iter().all(|p| *p == [0.0; 4]));
        assert_eq!(&pixels[WAVE_COLUMNS - 2..WAVE_COLUMNS], &[column(0), column(1)]);
        for i in 2..WAVE_COLUMNS * 3 + 7 {
            wave.push_column(column(i));
        }
        wave.load(&mut pixels);
        for (i, p) in pixels[..WAVE_COLUMNS].iter().enumerate() {
            assert_eq!(*p, column(WAVE_COLUMNS * 2 + 7 + i));
        }
        assert!(pixels[WAVE_COLUMNS..].iter().all(|p| *p == [0.0; 4]));
    }

    #[test]
    fn wave_samples_keep_the_latest_window_across_chunks() {
        let wave = SharedWave::default();
        let mut pixels = [[0.0; 4]; WAVE_COLUMNS * WAVE_ROWS];
        wave.push_samples(&[-1.0, 0.5]);
        wave.push_samples(&[]);
        wave.load(&mut pixels);
        assert!(pixels[..WAVE_COLUMNS * 2 - 2].iter().all(|p| *p == [0.0; 4]));
        assert_eq!(pixels[WAVE_COLUMNS * 2 - 2], [-1.0, 0.0, 0.0, 0.0]);
        assert_eq!(pixels[WAVE_COLUMNS * 2 - 1], [0.5, 0.0, 0.0, 0.0]);
        let samples: Vec<f32> = (0..WAVE_COLUMNS * 3 + 5).map(|i| (i as f32 * 0.1).sin()).collect();
        for chunk in samples.chunks(700) {
            wave.push_samples(chunk);
        }
        wave.load(&mut pixels);
        // the scope starts where the wave last rose through zero, within the last two windows
        let row: Vec<f32> = pixels[WAVE_COLUMNS..2 * WAVE_COLUMNS].iter().map(|p| p[0]).collect();
        let recent = &samples[samples.len() - 2 * WAVE_COLUMNS..];
        let start = (1..=WAVE_COLUMNS).rev().find(|&i| recent[i - 1] <= 0.0 && recent[i] > 0.0).unwrap();
        assert_eq!(row, &recent[start..start + WAVE_COLUMNS]);
        assert!(start > WAVE_COLUMNS - 70, "the latest rise: sin(i / 10) rises every 63 samples");
    }

    #[test]
    fn the_spectrum_row_lights_the_band_of_a_tone() {
        let wave = SharedWave::default();
        // a 1 kHz tone at 48 kHz, in the bins of a 512-point FFT
        let mut power = [1e-9f32; 257];
        power[11] = 1.0;
        for _ in 0..20 {
            wave.push_spectrum(&power, 48_000.0);
        }
        let mut pixels = [[0.0; 4]; WAVE_COLUMNS * WAVE_ROWS];
        wave.load(&mut pixels);
        let row: Vec<f32> = pixels[2 * WAVE_COLUMNS..].iter().map(|p| p[0]).collect();
        let loudest = (0..WAVE_COLUMNS).max_by(|&a, &b| row[a].total_cmp(&row[b])).unwrap();
        let hz = LOWEST_HZ * 2f32.powf((HIGHEST_HZ / LOWEST_HZ).log2() * loudest as f32 / WAVE_COLUMNS as f32);
        assert!((900.0..1200.0).contains(&hz), "the loudest band is the tone's: {hz} Hz");
        assert!(row[10] < 0.1 && row[500] < 0.1, "the rest stays dark");
        // and it falls back slowly once the tone stops
        wave.push_spectrum(&[1e-9; 257], 48_000.0);
        wave.load(&mut pixels);
        assert!(pixels[2 * WAVE_COLUMNS + loudest][0] > 0.5 * row[loudest]);
    }

    #[test]
    fn wave_counters_can_overflow() {
        let wave = SharedWave::default();
        wave.written.store(usize::MAX, Relaxed);
        wave.sampled.store(usize::MAX, Relaxed);
        wave.push_column([0.25; 4]);
        wave.push_column([0.5; 4]);
        wave.push_samples(&[-1.0, 1.0]);
        let mut pixels = [[0.0; 4]; WAVE_COLUMNS * WAVE_ROWS];
        wave.load(&mut pixels);
        assert_eq!(&pixels[WAVE_COLUMNS - 2..WAVE_COLUMNS], &[[0.25; 4], [0.5; 4]]);
        assert_eq!(pixels[WAVE_COLUMNS * 2 - 2][0], -1.0);
        assert_eq!(pixels[WAVE_COLUMNS * 2 - 1][0], 1.0);
    }

    #[test]
    fn the_render_thread_draws_a_graph_into_the_preview() {
        let _gpu = crate::gpu_lock();
        let Ok(engine) = Engine::start(None) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let device = engine.device();
        let program = |id: &str| Program::new(device, ShaderNode::parse(BUILTINS.iter().find(|b| b.0 == id).unwrap().1).unwrap()).unwrap();
        let programs = vec![program("gradient"), program("kaleido")];
        let nodes = [NodeSpec { program: 0, inputs: vec![] }, NodeSpec { program: 1, inputs: vec![Some(0)] }];
        let runner = Runner::new(device, &programs, &nodes, &[], [320, 180], engine.wave_texture()).unwrap();
        for n in 0..2 {
            runner.set_params(engine.queue(), &programs, n, &[]);
        }
        // kaleido's zoom (0.25..4, base 1) follows the kick
        let kaleido = &programs[1].node;
        let kick = velo_core::Mapping { param: "zoom".into(), source: "kick".into(), amount: 0.2, attack_ms: 5.0, release_ms: 150.0, invert: false };
        let mods = vec![Modulator::new(&programs[0].node, &[], Vec::new()), Modulator::new(kaleido, &kaleido.values(|_| None), crate::routes(kaleido, &[kick], &[]))];
        engine.send(Command::Install { key: 1, graph: Box::new(Graph { programs, runner, output: Some(1), mods, macros: Macros::default(), signals: Vec::new(), probes: Vec::new(), media: Vec::new(), lists: Vec::new(), lazy: Vec::new() }) });
        engine.send(Command::Launch { scene: 1, transition: Transition::Cut, duration_beats: 0.0, quantize: Quantize::Off });
        engine.send(Command::Edit(1));
        engine.send(Command::Tempo(140.0));
        engine.features.store([0.5; 12]);
        // the thread ticks at 60 Hz without an output: wait for a preview frame
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut lit = None;
        while lit.is_none() && Instant::now() < deadline {
            engine.take_preview(|pixels| lit = Some(pixels.as_chunks::<4>().0.iter().filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8).count()));
            std::thread::sleep(Duration::from_millis(10));
        }
        let lit = lit.expect("a preview frame within 5 s");
        assert!(lit > (PREVIEW[0] * PREVIEW[1] / 4) as usize, "the preview shows the picture: {lit} lit pixels");
        assert!(engine.status.frames.load(Relaxed) > 0);
        // the render thread modulates and publishes what it computed for the watched node
        let zoom = 2 * 2; // the third parameter's x
        let mut seen = None;
        while Instant::now() < deadline && seen.is_none_or(|v: [f32; 32]| (v[zoom] - 1.375).abs() > 1e-3) {
            seen = engine.status.watch(1);
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen.is_some_and(|v| (v[zoom] - (1.0 + 0.2 * 3.75 * 0.5)).abs() < 1e-3), "zoom at base + amount × range × kick: {seen:?}");
        // macro 0 drives kaleido's spin from -1 to 1; at 0.75 it's 0.5, then a morph takes it to 1
        let spin = 2; // the second parameter's x
        engine.send(Command::MacroTargets { key: 1, routes: vec![crate::MacroRoute { index: 0, node: 1, param: 1, axis: 0, min: -1.0, max: 1.0 }] });
        engine.send(Command::Macro { key: 1, index: 0, value: 0.75 });
        let near = |want: f32| {
            let mut seen = None;
            while Instant::now() < deadline && seen.is_none_or(|v: [f32; 32]| (v[spin] - want).abs() > 1e-3) {
                seen = engine.status.watch(1);
                std::thread::sleep(Duration::from_millis(10));
            }
            seen.map(|v| v[spin])
        };
        assert!(near(0.5).is_some_and(|v| (v - 0.5).abs() < 1e-3), "the macro sets spin's base");
        engine.send(Command::Morph { key: 1, to: [1.0; MACROS], beats: 0.5 });
        assert!(near(1.0).is_some_and(|v| (v - 1.0).abs() < 1e-3), "a morph of half a beat at 140 BPM gets there");
        assert_eq!(engine.status.watch(0), None, "the generator isn't mapped: nothing published for it");
        assert_eq!(engine.status.clock()[0], 140.0);
    }

    /// A graph of one Gradient node in one flat color (display values).
    fn flat(engine: &Engine, rgb: [f32; 3]) -> Box<Graph> {
        let device = engine.device();
        let program = Program::new(device, ShaderNode::parse(BUILTINS.iter().find(|b| b.0 == "gradient").unwrap().1).unwrap()).unwrap();
        let values = program.node.values(|p| p.starts_with("color").then_some([rgb[0], rgb[1], rgb[2], 1.0]));
        let runner = Runner::new(device, std::slice::from_ref(&program), &[NodeSpec { program: 0, inputs: vec![] }], &[], [64, 36], engine.wave_texture()).unwrap();
        runner.set_params(engine.queue(), std::slice::from_ref(&program), 0, &values);
        let mods = vec![Modulator::new(&program.node, &values, Vec::new())];
        Box::new(Graph { programs: vec![program], runner, output: Some(0), mods, macros: Macros::default(), signals: Vec::new(), probes: Vec::new(), media: Vec::new(), lists: Vec::new(), lazy: Vec::new() })
    }

    /// The newest preview's average red and blue (0..255), waiting up to 2 s for one.
    fn red_blue(engine: &Engine) -> Option<[f32; 2]> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut seen = None;
        while seen.is_none() && Instant::now() < deadline {
            engine.take_preview(|px| {
                let n = (px.len() / 4) as f32;
                let sum = |c: usize| px.iter().skip(c).step_by(4).map(|&v| f32::from(v)).sum::<f32>() / n;
                seen = Some([sum(0), sum(2)]);
            });
            std::thread::sleep(Duration::from_millis(5));
        }
        seen
    }

    #[test]
    fn the_test_pattern_marks_the_edge_and_the_middle_and_shows_its_bars() {
        let _gpu = crate::gpu_lock();
        let Ok(engine) = Engine::start(None) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let (device, queue) = (engine.device(), engine.queue());
        let [w, h] = [320u32, 180];
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let black = crate::gpu::texture(device, [1, 1]);
        let mut encoder = device.create_command_encoder(&Default::default());
        Blit::new(device, wgpu::TextureFormat::Rgba8Unorm).draw(device, queue, &mut encoder, [&black, &black], (0.0, PATTERN), &target.create_view(&Default::default()), None);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(w * h * 4), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let layout = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: None };
        encoder.copy_texture_to_buffer(target.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buffer, layout }, target.size());
        queue.submit([encoder.finish()]);
        buffer.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let data = buffer.slice(..).get_mapped_range().expect("mapped").to_vec();
        let at = |x: u32, y: u32| -> [u8; 3] { data[((y * w + x) * 4) as usize..][..3].try_into().unwrap() };
        let cyan = |p: [u8; 3]| p[0] < 80 && p[1] > 180 && p[2] > 200;
        assert!(cyan(at(0, 90)) && cyan(at(319, 90)) && cyan(at(160, 0)), "the edge: {:?}", at(0, 90));
        assert!(cyan(at(160, 90)), "the middle: {:?}", at(160, 90));
        assert_eq!(at(103, 119), [255, 255, 255], "the first bar is white");
        assert_eq!(at(119, 119), [255, 255, 0], "the second is yellow");
    }

    #[test]
    fn a_launch_crossfades_the_program_on_the_beat_while_the_preview_shows_the_edited_graph() {
        let _gpu = crate::gpu_lock();
        let Ok(engine) = Engine::start(None) else {
            eprintln!("no GPU adapter here: skipped");
            return;
        };
        let (red, blue, green) = (1, 2, 3);
        engine.send(Command::Install { key: red, graph: flat(&engine, [1.0, 0.0, 0.0]) });
        engine.send(Command::Install { key: blue, graph: flat(&engine, [0.0, 0.0, 1.0]) });
        engine.send(Command::Launch { scene: red, transition: Transition::Cut, duration_beats: 0.0, quantize: Quantize::Off });
        engine.send(Command::Tempo(120.0));
        let wait = |what: &str, ok: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !ok() {
                assert!(Instant::now() < deadline, "{what} within 5 s");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        let status = &engine.status;
        wait("red live", &|| status.live.load(Relaxed) == red);
        // (waits on what the picture shows: a slow GPU hands frames back late)
        wait("the program red", &|| red_blue(&engine).is_some_and(|[r, b]| r > 250.0 && b < 5.0));
        // blue, crossfaded over 2 beats (1 s) from the next beat
        engine.send(Command::Launch { scene: blue, transition: Transition::Crossfade, duration_beats: 2.0, quantize: Quantize::Beat });
        wait("the launch armed or under way", &|| status.armed().is_some_and(|a| a.0 == blue) || status.fading.load(Relaxed) == red);
        wait("the crossfade", &|| status.fading.load(Relaxed) == red);
        assert_eq!(status.live.load(Relaxed), blue, "blue is live from the fade's start");
        // a picture from the middle of it: both at once
        std::thread::sleep(Duration::from_millis(400));
        let [r, b] = red_blue(&engine).unwrap();
        assert!(r > 40.0 && r < 215.0 && b > 40.0 && b < 215.0, "half red, half blue: {r} {b}");
        wait("the end of the fade", &|| status.fading.load(Relaxed) == 0);
        wait("the program blue", &|| red_blue(&engine).is_some_and(|[r, b]| r < 5.0 && b > 250.0));
        // editing another scene: the preview shows it, the program stays blue
        engine.send(Command::Install { key: green, graph: flat(&engine, [0.0, 1.0, 0.0]) });
        engine.send(Command::Edit(green));
        wait("the preview the edited (green) graph", &|| red_blue(&engine).is_some_and(|[r, b]| r < 5.0 && b < 5.0));
        assert_eq!(status.live.load(Relaxed), blue);
        // a launch of a graph that isn't built yet waits for it
        engine.send(Command::Launch { scene: 9, transition: Transition::Cut, duration_beats: 0.0, quantize: Quantize::Off });
        wait("armed, building", &|| status.armed() == Some((9, 0.0)));
        assert_eq!(status.live.load(Relaxed), blue, "still blue while it builds");
        engine.send(Command::Install { key: 9, graph: flat(&engine, [1.0, 1.0, 1.0]) });
        wait("the launch once built", &|| status.live.load(Relaxed) == 9);
    }
}
