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
    /// The first device whose name contains this.
    Named(String),
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
    pub rate: f32,
    pub channels: u16,
    pub name: String,
    /// One app's tap, taken down after the stream (fields drop in order).
    pub tap: Option<crate::apps::Tap>,
    /// The stream said it can't go on (the device was unplugged or disabled): open it again.
    pub broken: Arc<AtomicBool>,
}

impl Capture {
    /// The default device this capture follows (the computer's sound, the default input) is now
    /// another one: open it again to hear that one.
    pub fn default_moved(&self, source: &Source) -> bool {
        let host = cpal::default_host();
        let now = match source {
            Source::Computer => host.default_output_device(),
            Source::DefaultInput => host.default_input_device(),
            _ => return false,
        };
        now.and_then(|d| d.description().ok()).is_some_and(|d| d.name() != self.name)
    }
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
        Source::Named(name) => named(name),
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
    let open = |buffer: cpal::BufferSize| {
        // a second of audio: if analysis stalls, the oldest audio is dropped, nothing waits
        let (tx, samples) = rtrb::RingBuffer::new(rate as usize);
        let stream_config = cpal::StreamConfig { buffer_size: buffer, ..config.into() };
        let stream = match config.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, stream_config, tx, broken.clone()),
            SampleFormat::I16 => build::<i16>(&device, stream_config, tx, broken.clone()),
            SampleFormat::I32 => build::<i32>(&device, stream_config, tx, broken.clone()),
            other => Err(format!("{other} samples aren't supported yet")),
        }?;
        Ok::<_, String>((stream, samples))
    };
    let (stream, samples) = short.map_or_else(|| open(cpal::BufferSize::Default), |b| open(b).or_else(|_| open(cpal::BufferSize::Default)))?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Capture { stream, samples, rate, channels, name, tap, broken })
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

fn build<T: SizedSample>(device: &cpal::Device, config: cpal::StreamConfig, mut tx: rtrb::Producer<f32>, broken: Arc<AtomicBool>) -> Result<cpal::Stream, String>
where
    f32: FromSample<T>,
{
    let channels = usize::from(config.channels).max(1);
    let scale = 1.0 / channels as f32;
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                for frame in data.chunks_exact(channels) {
                    let mono = frame.iter().map(|&s| f32::from_sample(s)).sum::<f32>() * scale;
                    // a NaN would stay in every filter after it: silence instead
                    let mono = if mono.is_finite() { mono } else { 0.0 };
                    // full: analysis is behind; drop rather than wait
                    let _ = tx.push(mono);
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use cpal::traits::{DeviceTrait, HostTrait};

    /// An app's tap makes an input cpal sees and can read the format of. Run by hand
    /// (`--ignored`) with an app open that has played sound: it never starts the audio, so
    /// it asks for no permission and plays nothing.
    #[test]
    #[ignore]
    fn an_apps_tap_is_an_input() {
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
