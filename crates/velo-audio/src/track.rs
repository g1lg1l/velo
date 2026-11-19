//! An audio file as the input (docs/audio.md §1): an MP3 decoded whole, played on the default
//! output over and over, and heard from where the output is in it, so the picture is in time with
//! what you hear. With no output (or none wanted), a clock plays it silently.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

/// A decoded file: its samples, the channels side by side, at its own rate.
pub struct Track {
    pub samples: Vec<i16>,
    pub channels: usize,
    pub rate: f32,
    pub name: String,
}

impl Track {
    /// The file decoded; why not, in words for the top bar.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if !nanomp3::detect(&bytes) {
            return Err(format!("{name} isn't an MP3: that's the kind of audio file Velo plays"));
        }
        let decoded = nanomp3::decode_all::<i16>(&bytes);
        let channels = match decoded.channels {
            Some(nanomp3::Channels::Mono) => 1,
            _ => 2,
        };
        if decoded.samples.len() < channels || decoded.sample_rate == 0 {
            return Err(format!("{name} has no sound in it"));
        }
        Ok(Self { samples: decoded.samples, channels, rate: decoded.sample_rate as f32, name })
    }

    /// How many frames (a sample a channel) it holds.
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    /// `out` filled with its frames from `at` on, as left and right (a mono file on both): what
    /// a recording takes.
    pub fn stereo(&self, at: usize, out: &mut [[f32; 2]]) {
        let (frames, right) = (self.frames(), usize::from(self.channels > 1));
        for (i, o) in out.iter_mut().enumerate() {
            let f = (at + i) % frames * self.channels;
            *o = [f32::from(self.samples[f]), f32::from(self.samples[f + right])].map(|s| s / 32_768.0);
        }
    }

    /// `out` filled with its frames from `at` on, the channels mixed to one: what the analysis hears.
    pub fn mono(&self, at: usize, out: &mut [f32]) {
        let (frames, scale) = (self.frames(), 1.0 / (32_768.0 * self.channels as f32));
        for (i, o) in out.iter_mut().enumerate() {
            let f = (at + i) % frames * self.channels;
            *o = self.samples[f..f + self.channels].iter().map(|&s| f32::from(s)).sum::<f32>() * scale;
        }
    }
}

/// A track playing in a loop: on the default output, or silently on a clock.
pub struct Player {
    _stream: Option<cpal::Stream>,
    /// The frames the output has played so far (as an f64's bits; never wrapped).
    played: Arc<AtomicU64>,
    start: Instant,
    rate: f32,
    /// The frame it started from.
    first: f64,
}

/// When the last player started, and how far into its track (seconds): what a film of the app
/// needs to put the same sound under its picture.
pub static STARTED: std::sync::Mutex<Option<(Instant, f64)>> = std::sync::Mutex::new(None);

impl Player {
    /// From `from` seconds in. `aloud`: on the default output device; false (or no such device)
    /// keeps it silent.
    pub fn start(track: Arc<Track>, aloud: bool, from: f64) -> Self {
        let first = from.max(0.0) * f64::from(track.rate);
        let played = Arc::new(AtomicU64::new(first.to_bits()));
        let stream = aloud.then(|| output(track.clone(), played.clone(), first).map_err(|e| eprintln!("velo-audio: the file plays silently: {e}")).ok()).flatten();
        let start = Instant::now();
        if let Ok(mut started) = STARTED.lock() {
            *started = Some((start, from.max(0.0)));
        }
        Self { _stream: stream, played, start, rate: track.rate, first }
    }

    /// The frames played so far, the ones it skipped at the start with them: the output's own
    /// count, or the clock's.
    pub fn position(&self) -> usize {
        match self._stream {
            Some(_) => f64::from_bits(self.played.load(Relaxed)) as usize,
            None => (self.first + self.start.elapsed().as_secs_f64() * f64::from(self.rate)) as usize,
        }
    }
}

fn output(track: Arc<Track>, played: Arc<AtomicU64>, first: f64) -> Result<cpal::Stream, String> {
    let device = cpal::default_host().default_output_device().ok_or("no audio output")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config.into(), track, played, first),
        SampleFormat::I16 => build::<i16>(&device, config.into(), track, played, first),
        SampleFormat::I32 => build::<i32>(&device, config.into(), track, played, first),
        other => Err(format!("{other} samples aren't supported yet")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: cpal::StreamConfig, track: Arc<Track>, played: Arc<AtomicU64>, first: f64) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels).max(1);
    // the file's rate into the device's: a step through the file for each frame out
    let step = f64::from(track.rate) / f64::from(config.sample_rate);
    let (frames, mut at) = (track.frames(), first);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_exact_mut(channels) {
                    // between the two frames it falls on, each channel its own (a mono file on both sides)
                    let (i, k) = (at as usize % frames, at.fract() as f32);
                    let j = (i + 1) % frames;
                    for (c, out) in frame.iter_mut().enumerate() {
                        let side = c.min(track.channels - 1);
                        let (a, b) = (f32::from(track.samples[i * track.channels + side]), f32::from(track.samples[j * track.channels + side]));
                        *out = T::from_sample((a + (b - a) * k) / 32_768.0);
                    }
                    at += step;
                }
                played.store(at.to_bits(), Relaxed);
            },
            |e| eprintln!("velo-audio: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_is_heard_as_one_channel_and_goes_round() {
        // a stereo file of three frames: left and right, mixed, and the fourth frame the first again
        let track = Track { samples: vec![16_384, 0, 0, -16_384, 32_767, 32_767], channels: 2, rate: 44_100.0, name: "t".into() };
        let mut heard = [0.0f32; 4];
        track.mono(0, &mut heard);
        assert_eq!(track.frames(), 3);
        assert!((heard[0] - 0.25).abs() < 1e-4 && (heard[1] + 0.25).abs() < 1e-4 && (heard[2] - 1.0).abs() < 1e-3, "{heard:?}");
        assert_eq!(heard[3], heard[0], "round again");
        // silent, a clock plays it: it has gone on a little by now
        let player = Player::start(Arc::new(track), false, 1.0);
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!((44_500..60_000).contains(&player.position()), "a second in, and a little more: {}", player.position());
        // what isn't an MP3 says so
        let path = std::env::temp_dir().join("velo-not-an-mp3.txt");
        std::fs::write(&path, "words").unwrap();
        assert!(Track::load(&path).err().is_some_and(|e| e.contains("isn't an MP3")));
        assert!(Track::load(std::path::Path::new("/nowhere/at/all.mp3")).is_err());
    }
}
