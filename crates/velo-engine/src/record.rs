//! Recording the program to a movie: the frames the outputs show, read back like a share's
//! (runtime.rs), go to ffmpeg at the project's frame rate as H.264 (the GPU's encoder on a Mac);
//! the audio Velo hears ([`Tap`], handed over by the audio thread) is kept in a WAV beside it and
//! put in when the recording stops, as late as the speakers play it. ffmpeg isn't bundled: the
//! one the user installed is found (Homebrew's, MacPorts', or on the PATH).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

/// What to record, and where.
#[derive(Clone)]
pub struct Spec {
    pub path: PathBuf,
    /// The canvas's size and frame rate: the movie's, whatever is live when it starts.
    pub size: [u32; 2],
    pub fps: u32,
    /// The audio, when it's wanted: what the audio thread hands over.
    pub audio: Option<Arc<Tap>>,
}

/// Where the audio thread hands over what it hears, while a recording takes it (nothing is
/// copied otherwise).
#[derive(Default)]
pub struct Tap {
    to: Mutex<Option<mpsc::Sender<Heard>>>,
}

/// Audio as it came: when, its rate, how late the pictures react to it, its frames.
struct Heard {
    at: Instant,
    rate: u32,
    lag: f32,
    frames: Vec<[f32; 2]>,
}

impl Tap {
    /// Whether a recording takes the audio now.
    pub fn wanted(&self) -> bool {
        self.to.lock().is_ok_and(|to| to.is_some())
    }

    /// `frames` (left, right) just heard at `rate`; the pictures follow them `lag` seconds late
    /// (speakers that lag, Settings → Speaker lag), and so does the recording's sound.
    pub fn push(&self, rate: f32, lag: f32, frames: &[[f32; 2]]) {
        if let Ok(to) = self.to.lock()
            && let Some(to) = &*to
        {
            let _ = to.send(Heard { at: Instant::now(), rate: rate as u32, lag, frames: frames.to_vec() });
        }
    }
}

/// ffmpeg, where it's installed: an app opened from the Finder doesn't get the shell's PATH, so
/// Homebrew's and MacPorts' places are looked in too.
pub fn ffmpeg() -> Option<PathBuf> {
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    let path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    path.into_iter().chain(["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"].map(PathBuf::from)).map(|dir| dir.join(name)).find(|p| p.is_file())
}

fn command(ffmpeg: &Path) -> Command {
    let mut c = Command::new(ffmpeg);
    c.args(["-hide_banner", "-loglevel", "error", "-y"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    c
}

/// A WAV of 32-bit float stereo, its sizes written when it's closed.
struct Wav {
    file: BufWriter<File>,
    path: PathBuf,
    rate: u32,
    frames: u64,
    /// When its first frame was heard, and the speakers' lag then.
    start: Instant,
    lag: f32,
}

impl Wav {
    fn create(path: PathBuf, rate: u32, start: Instant, lag: f32) -> std::io::Result<Self> {
        let mut wav = Self { file: BufWriter::new(File::create(&path)?), path, rate, frames: 0, start, lag };
        wav.header()?;
        Ok(wav)
    }

    fn header(&mut self) -> std::io::Result<()> {
        let data = (self.frames * 8).min(u64::from(u32::MAX - 36)) as u32;
        let mut h = Vec::with_capacity(44);
        h.extend(b"RIFF");
        h.extend((36 + data).to_le_bytes());
        h.extend(b"WAVEfmt ");
        // 16 bytes of format: IEEE float, 2 channels, the rate, bytes a second, a frame, bits
        h.extend(16u32.to_le_bytes());
        h.extend(3u16.to_le_bytes());
        h.extend(2u16.to_le_bytes());
        h.extend(self.rate.to_le_bytes());
        h.extend((self.rate * 8).to_le_bytes());
        h.extend(8u16.to_le_bytes());
        h.extend(32u16.to_le_bytes());
        h.extend(b"data");
        h.extend(data.to_le_bytes());
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&h)?;
        self.file.seek(SeekFrom::End(0))?;
        Ok(())
    }

    fn write(&mut self, frames: &[[f32; 2]]) -> std::io::Result<()> {
        for f in frames {
            self.file.write_all(&f[0].to_le_bytes())?;
            self.file.write_all(&f[1].to_le_bytes())?;
        }
        self.frames += frames.len() as u64;
        Ok(())
    }

    /// Up to where the clock says it should be by `now`: a stalled input (nothing came) is
    /// silence, so the sound never drifts ahead of the pictures.
    fn fill_to(&mut self, now: Instant) -> std::io::Result<()> {
        let due = (now.duration_since(self.start).as_secs_f64() * f64::from(self.rate)) as u64;
        // a little late is the input's own buffering, not a gap
        let gap = due.saturating_sub(self.frames + u64::from(self.rate) / 10);
        if gap > 0 {
            self.write(&vec![[0.0; 2]; gap as usize])?;
        }
        Ok(())
    }

    fn close(mut self) -> std::io::Result<PathBuf> {
        self.header()?;
        self.file.flush()?;
        Ok(self.path)
    }
}

/// A recording under way: what the share's thread sends each frame to.
pub struct Recorder {
    spec: Spec,
    ffmpeg: PathBuf,
    /// ffmpeg encoding the frames, and the file it writes (the movie itself with no sound, else
    /// a part put together with the sound at the end); started with the first frame.
    video: Option<(Child, PathBuf)>,
    size: [u32; 2],
    /// When the first frame came, and how many have gone out: a frame rate held whatever the
    /// render's (a late frame is sent again, an early one skipped).
    first: Option<Instant>,
    written: u64,
    rows: Vec<u8>,
    heard: Option<mpsc::Receiver<Heard>>,
    wav: Option<Wav>,
    failed: Option<String>,
    /// What came of it, for the interface: the movie, or why not.
    told: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

impl Recorder {
    /// `told` gets the movie's path when it's done, or why it failed.
    pub fn new(spec: Spec, told: Arc<Mutex<Option<Result<PathBuf, String>>>>) -> Result<Self, String> {
        let ffmpeg = ffmpeg().ok_or("Recording needs ffmpeg: install it (brew install ffmpeg, winget install ffmpeg, or your system's package)")?;
        let heard = spec.audio.as_ref().map(|tap| {
            let (to, heard) = mpsc::channel();
            if let Ok(mut slot) = tap.to.lock() {
                *slot = Some(to);
            }
            heard
        });
        Ok(Self { spec, ffmpeg, video: None, size: [0, 0], first: None, written: 0, rows: Vec::new(), heard, wav: None, failed: None, told })
    }

    /// A file beside the movie for one of its parts while it's recorded.
    fn part(&self, ext: &str) -> PathBuf {
        let mut name = self.spec.path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".part.{ext}"));
        self.spec.path.with_file_name(name)
    }

    fn start_video(&mut self, [w, h]: [u32; 2]) -> Result<(), String> {
        let out = if self.heard.is_some() { self.part("mp4") } else { self.spec.path.clone() };
        let fps = self.spec.fps.max(1);
        let mut c = command(&self.ffmpeg);
        c.args(["-f", "rawvideo", "-pix_fmt", "bgra", "-s", &format!("{w}x{h}"), "-framerate", &fps.to_string(), "-i", "-"]);
        // 4:2:0 needs even sides
        c.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-pix_fmt", "yuv420p"]);
        if cfg!(target_os = "macos") {
            // the GPU's encoder, at about 0.15 bits a pixel
            let bits = (f64::from(w) * f64::from(h) * f64::from(fps) * 0.15) as u64;
            c.args(["-c:v", "h264_videotoolbox", "-b:v", &bits.to_string()]);
        } else {
            c.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "18"]);
        }
        c.args(["-movflags", "+faststart"]).arg(&out);
        let child = c.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ffmpeg didn't start: {e}"))?;
        self.video = Some((child, out));
        self.size = [w, h];
        Ok(())
    }

    /// What the audio thread handed over since the last frame, into the WAV.
    fn take_audio(&mut self) -> std::io::Result<()> {
        let Some(heard) = &self.heard else { return Ok(()) };
        for h in heard.try_iter() {
            if self.wav.is_none() {
                // the first frame was heard as long before this as the chunk lasts
                let start = h.at.checked_sub(std::time::Duration::from_secs_f64(h.frames.len() as f64 / f64::from(h.rate.max(1)))).unwrap_or(h.at);
                self.wav = Some(Wav::create(self.part("wav"), h.rate, start, h.lag)?);
            }
            if let Some(wav) = &mut self.wav {
                wav.write(&h.frames)?;
            }
        }
        if let Some(wav) = &mut self.wav {
            wav.fill_to(Instant::now())?;
        }
        Ok(())
    }

    fn frame(&mut self, pixels: &[u8], size: [u32; 2], stride: u32) -> Result<(), String> {
        self.take_audio().map_err(|e| format!("the sound couldn't be written: {e}"))?;
        if self.video.is_none() {
            self.start_video(size)?;
        }
        // (the engine reads the program back at the movie's size whatever the canvas becomes)
        if size != self.size {
            return Ok(());
        }
        let first = *self.first.get_or_insert_with(Instant::now);
        let due = (first.elapsed().as_secs_f64() * f64::from(self.spec.fps.max(1))) as u64 + 1;
        let [w, h] = size.map(|v| v as usize);
        let packed = if stride as usize == w * 4 {
            &pixels[..w * h * 4]
        } else {
            self.rows.clear();
            for row in pixels.chunks(stride as usize).take(h) {
                self.rows.extend_from_slice(&row[..w * 4]);
            }
            &self.rows[..]
        };
        let Some((child, _)) = &mut self.video else { return Ok(()) };
        let Some(stdin) = &mut child.stdin else { return Ok(()) };
        while self.written < due {
            stdin.write_all(packed).map_err(|_| "ffmpeg stopped taking frames".to_string())?;
            self.written += 1;
        }
        Ok(())
    }

    /// The video closed, the sound put in: the movie, or why not (what was recorded is kept).
    fn finish(&mut self) -> Result<PathBuf, String> {
        if let Some(tap) = &self.spec.audio
            && let Ok(mut slot) = tap.to.lock()
        {
            *slot = None;
        }
        let _ = self.take_audio();
        let Some((mut child, video)) = self.video.take() else {
            if let Some(wav) = self.wav.take().and_then(|w| w.close().ok()) {
                let _ = std::fs::remove_file(wav);
            }
            return Err("no frame came to record".into());
        };
        drop(child.stdin.take());
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(format!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        if let Some(e) = self.failed.take() {
            return Err(e);
        }
        let Some(wav) = self.wav.take() else {
            // asked for sound and none came: the movie without it
            if video != self.spec.path {
                std::fs::rename(&video, &self.spec.path).map_err(|e| e.to_string())?;
            }
            return Ok(self.spec.path.clone());
        };
        let (start, lag) = (wav.start, wav.lag);
        let wav = wav.close().map_err(|e| e.to_string())?;
        // the sound's place against the pictures: when its first frame and the first picture
        // came, and the speakers' lag (the pictures follow the sound that late, on purpose)
        let first = self.first.unwrap_or(start);
        let offset = if start >= first { start.duration_since(first).as_secs_f64() } else { -first.duration_since(start).as_secs_f64() } + f64::from(lag);
        let mut c = command(&self.ffmpeg);
        c.arg("-i").arg(&video);
        if offset >= 0.0 {
            c.args(["-itsoffset", &format!("{offset:.4}")]);
        } else {
            c.args(["-ss", &format!("{:.4}", -offset)]);
        }
        c.arg("-i").arg(&wav);
        c.args(["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-c:a", "aac", "-b:a", "256k", "-movflags", "+faststart"]).arg(&self.spec.path);
        let muxed = c.stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
        if !muxed.status.success() {
            // the pictures stay, under the movie's name; the sound beside them
            let _ = std::fs::rename(&video, &self.spec.path);
            return Err(format!("the sound couldn't be put in (kept beside the movie): {}", String::from_utf8_lossy(&muxed.stderr).trim()));
        }
        let _ = std::fs::remove_file(&video);
        let _ = std::fs::remove_file(&wav);
        Ok(self.spec.path.clone())
    }
}

impl crate::share::Sender for Recorder {
    fn send(&mut self, pixels: &[u8], size: [u32; 2], stride: u32) -> Result<(), String> {
        self.frame(pixels, size, stride).inspect_err(|e| self.failed = Some(e.clone()))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let result = self.finish();
        if let Ok(mut told) = self.told.lock() {
            *told = Some(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_says_its_size_and_fills_a_stalled_input_with_silence() {
        let dir = std::env::temp_dir().join(format!("velo-record-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let start = Instant::now() - std::time::Duration::from_secs(1);
        let mut wav = Wav::create(dir.join("a.wav"), 1000, start, 0.0).unwrap();
        wav.write(&[[0.5, -0.5]; 100]).unwrap();
        // a second gone and only a tenth of it heard: the rest is silence (less the buffering allowed)
        wav.fill_to(start + std::time::Duration::from_secs(1)).unwrap();
        assert_eq!(wav.frames, 900);
        let bytes = std::fs::read(wav.close().unwrap()).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 900 * 8, "the data's size");
        assert_eq!(bytes.len(), 44 + 900 * 8);
        assert_eq!(f32::from_le_bytes(bytes[44..48].try_into().unwrap()), 0.5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only where ffmpeg is installed: a second of pictures and sound makes a movie with both.
    #[test]
    fn a_recording_is_a_movie_with_its_sound() {
        let Some(ffmpeg) = ffmpeg() else {
            eprintln!("no ffmpeg here: skipped");
            return;
        };
        let dir = std::env::temp_dir().join(format!("velo-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("take.mp4");
        let tap = Arc::new(Tap::default());
        let told = Arc::new(Mutex::new(None));
        let mut recorder = Recorder::new(Spec { path: path.clone(), size: [64, 32], fps: 30, audio: Some(tap.clone()) }, told.clone()).unwrap();
        assert!(tap.wanted());
        // 64×32, rows padded to 256 bytes as a read-back's are
        let pixels = vec![200u8; 256 * 32];
        for _ in 0..30 {
            tap.push(48_000.0, 0.0, &[[0.1, -0.1]; 1600]);
            crate::share::Sender::send(&mut recorder, &pixels, [64, 32], 256).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
        drop(recorder);
        assert!(!tap.wanted(), "the audio thread lets go");
        assert_eq!(told.lock().unwrap().take(), Some(Ok(path.clone())));
        let probe = Command::new(ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" })).args(["-v", "error", "-show_entries", "stream=codec_type", "-of", "csv=p=0"]).arg(&path).output();
        if let Ok(probe) = probe {
            let streams = String::from_utf8_lossy(&probe.stdout);
            assert!(streams.contains("video") && streams.contains("audio"), "{streams}");
        }
        assert!(!dir.join("take.mp4.part.mp4").exists() && !dir.join("take.mp4.part.wav").exists(), "the parts are gone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
