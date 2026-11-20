//! Audio input (docs/audio.md §1): the computer's own output (Spotify, a browser, a DJ app), the
//! default input, or a device by name. The callback only downmixes to mono and pushes into a
//! lock-free ring; it never allocates, locks or waits.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};

/// Where the sound comes from.
pub enum Source {
    /// Whatever the computer is playing: loopback of the default output (WASAPI loopback on
    /// Windows, a Core Audio process tap on macOS 14.6+, the sink's monitor with PipeWire).
    Computer,
    DefaultInput,
    /// The first device whose name contains this, and the first of its channels heard (0 for
    /// 1–2, 2 for 3–4…): music on a multichannel interface often comes in on 3–4 or later.
    Named(String, u16),
    /// One app's sound ([`crate::apps`]): its bundle on disk, its name.
    App {
        id: String,
        name: String,
    },
}

/// An open input: keep `stream` alive, read mono samples from `samples`.
pub struct Capture {
    pub stream: cpal::Stream,
    pub samples: rtrb::Consumer<f32>,
    /// The same audio as left and right (the channels heard), for a recording to take.
    pub stereo: rtrb::Consumer<[f32; 2]>,
    pub rate: f32,
    pub channels: u16,
    pub name: String,
    /// One app's tap, taken down after the stream (fields drop in order).
    pub tap: Option<crate::apps::Tap>,
    /// The stream said it can't go on (the device was unplugged or disabled): open it again.
    pub broken: Arc<AtomicBool>,
}

/// The default device a capture of `source` named `name` follows (the computer's sound, the
/// default input) is now another one: open it again to hear that one.
pub fn default_moved(name: &str, source: &Source) -> bool {
    let host = cpal::default_host();
    let now = match source {
        Source::Computer => host.default_output_device(),
        Source::DefaultInput => host.default_input_device(),
        _ => return false,
    };
    now.and_then(|d| d.description().ok()).is_some_and(|d| d.name() != name)
}

/// The names of the input devices (for a device picker). Can take a moment: call it off the UI thread.
pub fn inputs() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices().map(|devices| devices.filter_map(|d| d.description().ok().map(|x| x.name().to_string())).collect()).unwrap_or_default()
}

pub fn open(source: &Source) -> Result<Capture, String> {
    let host = cpal::default_host();
    let named = |name: &str| host.devices().ok().and_then(|mut all| all.find(|d| d.description().is_ok_and(|x| x.name().contains(name))));
    let tap = match source {
        Source::App { id, name } => Some(crate::apps::Tap::new(id, name)?),
        _ => None,
    };
    let device = match source {
        Source::Computer => host.default_output_device(),
        Source::DefaultInput => host.default_input_device(),
        Source::Named(name, _) => named(name),
        // the tap's aggregate device: ours only, an input like any other
        Source::App { .. } => tap.as_ref().and_then(|t| appears(|| named(&t.device))),
    }
    .ok_or("no such audio device")?;
    let name = match source {
        Source::App { name, .. } => name.clone(),
        _ => device.description().map(|d| d.name().to_string()).unwrap_or_default(),
    };
    // an output device opened for input is its loopback (cpal does the platform part)
    let config = if device.supports_input() { device.default_input_config() } else { device.default_output_config() }.map_err(|e| e.to_string())?;
    let (rate, channels) = (config.sample_rate() as f32, config.channels());
    // a short buffer: the samples come every ~5 ms instead of whenever the device likes (often
    // 10 to 20 ms), the device's own size if it won't
    let short = match config.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => Some(cpal::BufferSize::Fixed(256.clamp(*min, *max))),
        cpal::SupportedBufferSize::Unknown => None,
    };
    let broken = Arc::new(AtomicBool::new(false));
    let first = match source {
        Source::Named(_, first) => usize::from(*first),
        _ => 0,
    };
    let open = |buffer: cpal::BufferSize| {
        // a second of audio: if analysis stalls, the newest audio is dropped, nothing waits
        let (tx, samples) = rtrb::RingBuffer::new(rate as usize);
        let (both, stereo) = rtrb::RingBuffer::new(rate as usize);
        let stream_config = cpal::StreamConfig { buffer_size: buffer, ..config.into() };
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, stream_config, first, (tx, both), broken.clone()),
            SampleFormat::I16 => build::<i16>(&device, stream_config, first, (tx, both), broken.clone()),
            SampleFormat::I32 => build::<i32>(&device, stream_config, first, (tx, both), broken.clone()),
            other => Err(format!("{other} samples aren't supported yet")),
        }?;
        Ok::<_, String>((stream, samples, stereo))
    };
    let (stream, samples, stereo) = short.map_or_else(|| open(cpal::BufferSize::Default), |b| open(b).or_else(|_| open(cpal::BufferSize::Default)))?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Capture { stream, samples, stereo, rate, channels, name, tap, broken })
}

/// A device just made (an app's tap) shows among the devices a moment later: up to a second.
fn appears(find: impl Fn() -> Option<cpal::Device>) -> Option<cpal::Device> {
    (0..20).find_map(|i| {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        find()
    })
}

/// The channels heard of a frame of `channels`: `first` and the one after it, the last one alone
/// when it has no pair, 1–2 when the device hasn't `first` (a pair saved for another device).
fn pair(channels: usize, first: usize) -> (usize, usize) {
    let first = if first < channels { first } else { 0 };
    (first, (first + 1).min(channels - 1))
}

/// An interleaved frame's channels `left` and `right` as mono and as left and right (one channel:
/// on both sides). A NaN would stay in every filter after it: silence instead.
pub(crate) fn pick<T: Sample>(frame: &[T], (left, right): (usize, usize)) -> (f32, [f32; 2])
where
    f32: FromSample<T>,
{
    let [l, r] = [frame[left], frame[right]].map(|s| f32::from_sample(s)).map(|s| if s.is_finite() { s } else { 0.0 });
    ((l + r) * 0.5, [l, r])
}

fn build<T: SizedSample>(device: &cpal::Device, config: cpal::StreamConfig, first: usize, (mut tx, mut both): (rtrb::Producer<f32>, rtrb::Producer<[f32; 2]>), broken: Arc<AtomicBool>) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    // a pair, not every channel: an interface's other inputs (often silent) would turn it down,
    // -12 dB on an 8-channel card
    let heard = pair(channels, first);
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks_exact(channels) {
                    let (mono, stereo) = pick(frame, heard);
                    // full: analysis is behind; drop rather than wait
                    let _ = tx.push(mono);
                    let _ = both.push(stereo);
                }
            },
            move |e| {
                // a glitch (an underrun, a route followed) goes on by itself; only a device or a
                // stream that's gone is opened again
                use cpal::ErrorKind::{DeviceNotAvailable, HostUnavailable, StreamInvalidated, Xrun};
                match e.kind() {
                    DeviceNotAvailable | HostUnavailable | StreamInvalidated => {
                        eprintln!("velo-audio: {e}");
                        broken.store(true, Relaxed);
                    }
                    Xrun => {}
                    _ => eprintln!("velo-audio: {e}"),
                }
            },
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{pair, pick};

    #[test]
    fn a_pair_of_channels_is_picked_from_interleaved_frames() {
        // two frames of five channels: channel c of frame f is f * 10 + c
        let frames: [[f32; 5]; 2] = std::array::from_fn(|f| std::array::from_fn(|c| (f * 10 + c) as f32));
        let heard = |first| frames.map(|frame| pick(&frame, pair(5, first)));
        assert_eq!(heard(0), [(0.5, [0.0, 1.0]), (10.5, [10.0, 11.0])], "1–2");
        assert_eq!(heard(2), [(2.5, [2.0, 3.0]), (12.5, [12.0, 13.0])], "3–4");
        assert_eq!(heard(4), [(4.0, [4.0, 4.0]), (14.0, [14.0, 14.0])], "5 alone, on both sides");
        assert_eq!(heard(6), heard(0), "a pair the device hasn't: 1–2");
        assert_eq!(pair(1, 0), (0, 0), "a mono device");
        assert_eq!(pick(&[f32::NAN, 0.5], (0, 1)), (0.25, [0.0, 0.5]), "a NaN is silence");
    }

    /// An app's tap makes an input cpal sees and can read the format of. Run by hand
    /// (`--ignored`) with an app open that has played sound: it never starts the audio, so
    /// it asks for no permission and plays nothing.
    #[test]
    #[ignore]
    #[cfg(target_os = "macos")]
    fn an_apps_tap_is_an_input() {
        use cpal::traits::{DeviceTrait, HostTrait};

        let Some(app) = crate::apps::apps().into_iter().next() else {
            eprintln!("no app has used audio: skipped");
            return;
        };
        let tap = crate::apps::Tap::new(&app.id, &app.name).unwrap();
        let host = cpal::default_host();
        let device = super::appears(|| host.devices().unwrap().find(|d| d.description().is_ok_and(|x| x.name() == tap.device))).expect("the tap's device");
        let config = device.default_input_config().unwrap();
        eprintln!("{}: {} via {} ({} ch, {} Hz)", app.name, tap.processes.len(), tap.device, config.channels(), config.sample_rate());
        assert!(config.channels() >= 1);
    }
}
