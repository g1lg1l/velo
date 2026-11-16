//! `VELO_REEL=out.mp4`, in a capture run (`VELO_CAPTURE_TO`, which ends it at `VELO_CAPTURE_AT`):
//! a recording of Velo being used. A script plays the pointer and the keyboard and writes
//! captions; the window's frames go to ffmpeg, 30 a second. No screen-recording permission, and
//! nobody at the mouse: the same film every time, to make again when the interface changes.
//!
//! The script (`VELO_REEL_SCRIPT`, or `scripts/reel.txt`) is a step a line, `seconds what`:
//! `move x y` (the pointer gets there then), `click`, `rclick`, `down`, `up`, `key Tab` (`key Cmd+J`), `type words`,
//! `scroll points` (the wheel, down), `say words` (a caption; alone, none; `say _words` puts it low, clear of a menu open at the
//! top), `zoom x y times` (the camera closes in on that point, `times` as near; `zoom 720 405 1`
//! is the whole window again). Points are the window's, in points.
//!
//! The camera's moves are made afterwards, by ffmpeg, from the filter the run leaves beside the
//! film (`.vf`): the frames go out whole. Captions are drawn where the camera will be looking.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Event, Key, PointerButton, Pos2, Stroke, pos2, vec2};

const FPS: f64 = 30.0;
/// How long the pointer takes to get where a `move` sends it, and a typed letter to follow the last.
const TRAVEL: f64 = 0.38;
const LETTER: f64 = 0.05;
/// How long the camera takes to get where a `zoom` sends it, and a caption to come in.
const GLIDE: f64 = 0.45;
const FADE: f64 = 0.2;
/// The window the film is made of, in points (reel.sh asks for it).
const SCREEN: egui::Vec2 = vec2(1440.0, 810.0);

enum Step {
    Move(Pos2),
    Click,
    RightClick,
    Down,
    Up,
    Key(Key, egui::Modifiers),
    Type(String),
    Say(String),
    Zoom(Pos2, f32),
    Scroll(f32),
}

pub struct Reel {
    out: std::path::PathBuf,
    script: Vec<(f64, Step)>,
    next: usize,
    pointer: Pos2,
    clicked: f64,
    caption: String,
    /// When the caption was said, and when the output window was first seen open.
    said: f64,
    opened: Option<f64>,
    /// The frame asked for last, once it's in.
    shot: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
    asked: bool,
    ffmpeg: Option<Child>,
    /// When the first frame came in, and how many went out since.
    start: f64,
    written: u64,
    /// When the app started (the test loop with it), and how long before the first frame.
    born: std::time::Instant,
    lead: f32,
    /// When the first frame came.
    first: std::time::Instant,
}

impl Reel {
    pub fn from_env() -> Option<Self> {
        let out = std::path::PathBuf::from(std::env::var_os("VELO_REEL")?);
        let path = std::env::var("VELO_REEL_SCRIPT").unwrap_or_else(|_| "scripts/reel.txt".into());
        let text = std::fs::read_to_string(&path).map_err(|e| eprintln!("velo: no reel script at {path}: {e}")).ok()?;
        let mut script = Vec::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let mut words = line.splitn(3, ' ');
            let (Some(at), Some(what)) = (words.next().and_then(|t| t.parse::<f64>().ok()), words.next()) else { continue };
            let rest = words.next().unwrap_or_default();
            match what {
                "move" => {
                    let xy: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if let [x, y] = xy[..] {
                        script.push((at, Step::Move(pos2(x, y))));
                    }
                }
                "click" => script.push((at, Step::Click)),
                "rclick" => script.push((at, Step::RightClick)),
                "down" => script.push((at, Step::Down)),
                "up" => script.push((at, Step::Up)),
                // "Tab", or with what's held: "Cmd+J", "Shift+Cmd+N"
                "key" => {
                    let mut held = egui::Modifiers::NONE;
                    let mut names = rest.split('+').rev();
                    let key = names.next().and_then(Key::from_name);
                    for name in names {
                        held |= match name {
                            "Cmd" | "Ctrl" => egui::Modifiers { command: true, mac_cmd: cfg!(target_os = "macos"), ctrl: !cfg!(target_os = "macos"), ..Default::default() },
                            "Shift" => egui::Modifiers::SHIFT,
                            "Alt" => egui::Modifiers::ALT,
                            _ => egui::Modifiers::NONE,
                        };
                    }
                    script.extend(key.map(|key| (at, Step::Key(key, held))));
                }
                "type" => script.extend(rest.chars().enumerate().map(|(i, c)| (at + i as f64 * LETTER, Step::Type(c.to_string())))),
                "say" => script.push((at, Step::Say(rest.to_string()))),
                // down by so many points, a little at a time over half a second
                "scroll" => script.extend(rest.parse::<f32>().ok().into_iter().flat_map(|by| (0..15).map(move |i| (at + f64::from(i) * 0.033, Step::Scroll(by / 15.0))))),
                "zoom" => {
                    let v: Vec<f32> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                    if let [x, y, times] = v[..] {
                        script.push((at, Step::Zoom(pos2(x, y), times.clamp(1.0, 2.0))));
                    }
                }
                _ => eprintln!("velo: reel: what is {line:?}?"),
            }
        }
        script.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(Self {
            out,
            script,
            next: 0,
            pointer: pos2(-100.0, -100.0),
            clicked: f64::NEG_INFINITY,
            caption: String::new(),
            said: 0.0,
            opened: None,
            shot: Arc::default(),
            asked: false,
            ffmpeg: None,
            start: 0.0,
            written: 0,
            born: std::time::Instant::now(),
            lead: 0.0,
            first: std::time::Instant::now(),
        })
    }

    /// Where the script has the pointer now: at the last place it was sent, or on its way to
    /// the next, easing in and out.
    fn pointer_at(&self, now: f64) -> Pos2 {
        let moves = self.script.iter().filter_map(|(at, step)| match step {
            Step::Move(p) => Some((*at, *p)),
            _ => None,
        });
        let (mut from, mut to) = (None, None);
        for (at, p) in moves {
            if at <= now {
                from = Some(p);
            } else {
                to = Some((at, p));
                break;
            }
        }
        match (from, to) {
            (from, Some((at, to))) if now > at - TRAVEL => {
                let k = ((now - (at - TRAVEL)) / TRAVEL) as f32;
                let from = from.unwrap_or(to + vec2(0.0, 120.0));
                from + (to - from) * (k * k * (3.0 - 2.0 * k))
            }
            (Some(from), _) => from,
            (None, _) => pos2(-100.0, -100.0),
        }
    }

    /// The camera's moves, each as (when it starts, by how much x, y and the zoom change): it
    /// eases from one `zoom` to the next over [`GLIDE`], and starts on the whole window.
    fn moves(&self) -> Vec<(f64, [f32; 3])> {
        let mut last = [SCREEN.x / 2.0, SCREEN.y / 2.0, 1.0];
        let mut out = Vec::new();
        for (at, step) in &self.script {
            if let Step::Zoom(p, times) = step {
                let to = [p.x, p.y, *times];
                out.push((at - GLIDE, [to[0] - last[0], to[1] - last[1], to[2] - last[2]]));
                last = to;
            }
        }
        out
    }

    /// What the camera sees at `now`, of the window: the same sums ffmpeg is given ([`Self::filter`]).
    fn view(&self, now: f64) -> egui::Rect {
        let mut v = [SCREEN.x / 2.0, SCREEN.y / 2.0, 1.0];
        for (from, by) in self.moves() {
            let k = (((now - from) / GLIDE) as f32).clamp(0.0, 1.0);
            let k = k * k * (3.0 - 2.0 * k);
            for (v, by) in v.iter_mut().zip(by) {
                *v += by * k;
            }
        }
        let size = SCREEN / v[2];
        let min = pos2((v[0] - size.x / 2.0).clamp(0.0, SCREEN.x - size.x), (v[1] - size.y / 2.0).clamp(0.0, SCREEN.y - size.y));
        egui::Rect::from_min_size(min, size)
    }

    /// The camera for ffmpeg: a zoompan over the whole frames, down to 1080p.
    fn filter(&self) -> String {
        let sum = |start: f32, i: usize| {
            let mut e = format!("{start}");
            for (from, by) in self.moves() {
                let k = format!("clip((it-{from:.3})/{GLIDE},0,1)");
                e += &format!("+({:.4})*{k}*{k}*(3-2*{k})", by[i]);
            }
            e
        };
        let (x, y, z) = (sum(SCREEN.x / 2.0, 0), sum(SCREEN.y / 2.0, 1), sum(1.0, 2));
        format!("zoompan=z='{z}':x='clip(({x})*iw/{}-iw/zoom/2,0,iw-iw/zoom)':y='clip(({y})*ih/{}-ih/zoom/2,0,ih-ih/zoom)':d=1:s=1920x1080:fps=30", SCREEN.x, SCREEN.y)
    }

    /// This frame's input: nothing of the real mouse and keys, what the script says instead.
    pub fn input(&mut self, raw: &mut egui::RawInput) {
        raw.events.clear();
        let now = raw.time.unwrap_or(0.0);
        self.pointer = self.pointer_at(now);
        raw.events.push(Event::PointerMoved(self.pointer));
        let button = |pressed| Event::PointerButton { pos: self.pointer, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        while let Some((_, step)) = self.script.get(self.next).filter(|(at, _)| *at <= now) {
            match step {
                Step::Move(_) => {}
                Step::Click => {
                    raw.events.extend([button(true), button(false)]);
                    self.clicked = now;
                }
                Step::RightClick => {
                    raw.events.extend([true, false].map(|pressed| Event::PointerButton { pos: self.pointer, button: PointerButton::Secondary, pressed, modifiers: Default::default() }));
                    self.clicked = now;
                }
                Step::Down => {
                    raw.events.push(button(true));
                    self.clicked = now;
                }
                Step::Up => raw.events.push(button(false)),
                Step::Key(key, held) => raw.events.extend([true, false].map(|pressed| Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: *held })),
                Step::Type(letters) => raw.events.push(Event::Text(letters.clone())),
                Step::Say(words) => {
                    self.caption.clone_from(words);
                    self.said = now;
                }
                Step::Zoom(..) => {}
                Step::Scroll(by) => raw.events.push(Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: vec2(0.0, -by), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Default::default() }),
            }
            self.next += 1;
        }
    }

    /// Each frame: the one asked for last goes out, the next is asked for; the pointer and the
    /// caption are drawn over everything, so they're in it.
    pub fn frame(&mut self, ctx: &egui::Context, picture: Option<egui::TextureId>, output: bool) {
        let now = ctx.input(|i| i.time);
        let shot = self.shot.lock().ok().and_then(|mut s| s.take());
        if let Some(image) = shot {
            self.asked = false;
            self.write(&image, now);
        }
        if !self.asked {
            self.asked = true;
            let slot = self.shot.clone();
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |image| {
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(image);
                }
            })));
        }
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("reel")));
        // the output window, which opens on another screen (off this one, in a capture run): shown
        // as a window over the canvas, the picture going out in it, so the film has it
        if output && self.opened.is_none() {
            self.opened = Some(now);
        }
        if let (Some(opened), Some(picture)) = (self.opened.filter(|_| output), picture) {
            let k = (((now - opened) / 0.45) as f32).clamp(0.0, 1.0);
            let k = 1.0 - (1.0 - k).powi(3);
            let size = vec2(520.0, 292.5 + 30.0) * (0.86 + 0.14 * k);
            let frame = egui::Rect::from_center_size(pos2(520.0, 520.0 + 24.0 * (1.0 - k)), size);
            let alpha = |c: Color32| c.gamma_multiply(k);
            painter.rect_filled(frame.expand(10.0).translate(vec2(0.0, 8.0)), 18.0, alpha(Color32::from_black_alpha(110)));
            painter.rect_filled(frame, 10.0, alpha(Color32::from_rgb(30, 32, 36)));
            let bar = egui::Rect::from_min_size(frame.min, vec2(frame.width(), 30.0));
            for (i, color) in [Color32::from_rgb(255, 95, 87), Color32::from_rgb(254, 188, 46), Color32::from_rgb(40, 200, 64)].into_iter().enumerate() {
                painter.circle_filled(bar.left_center() + vec2(18.0 + 20.0 * i as f32, 0.0), 6.0, alpha(color));
            }
            painter.text(bar.center(), egui::Align2::CENTER_CENTER, "Velo · Output", egui::FontId::proportional(13.0), alpha(Color32::from_gray(200)));
            let screen = egui::Rect::from_min_max(bar.left_bottom(), frame.max);
            painter.rect_filled(screen, egui::CornerRadius { nw: 0, ne: 0, sw: 10, se: 10 }, alpha(Color32::BLACK));
            painter.image(picture, screen.shrink(1.0), egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), alpha(Color32::WHITE));
            painter.rect_stroke(frame, 10.0, Stroke::new(1.0, alpha(Color32::from_white_alpha(46))), egui::StrokeKind::Inside);
        }
        // the caption: where the camera will be looking, as large there as anywhere, coming in
        if !self.caption.is_empty() {
            let view = self.view(now);
            let scale = view.width() / SCREEN.x;
            let k = (((now - self.said) / FADE) as f32).clamp(0.0, 1.0);
            let k = 1.0 - (1.0 - k).powi(3);
            let (words, y) = match self.caption.strip_prefix('_') {
                Some(words) => (words, view.bottom() - 236.0 * scale),
                None => (self.caption.as_str(), view.top() + 96.0 * scale),
            };
            let y = y + 10.0 * scale * (1.0 - k);
            let ink = Color32::WHITE.gamma_multiply(k);
            let galley = painter.layout_no_wrap(words.to_string(), egui::FontId::proportional(22.0 * scale), ink);
            let pill = egui::Rect::from_center_size(pos2(view.center().x - 20.0 * scale, y), galley.size() + vec2(44.0, 26.0) * scale);
            painter.rect_filled(pill, 14.0 * scale, Color32::from_rgba_unmultiplied(8, 10, 12, 235).gamma_multiply(k));
            painter.rect_stroke(pill, 14.0 * scale, Stroke::new(scale, Color32::from_white_alpha(40).gamma_multiply(k)), egui::StrokeKind::Inside);
            painter.galley(pill.center() - galley.size() / 2.0, galley, ink);
        }
        // a click's ripple, then the arrow: white, edged dark, as a pointer is
        let since = (now - self.clicked) as f32;
        if (0.0..0.4).contains(&since) {
            let k = since / 0.4;
            painter.circle_stroke(self.pointer, 6.0 + 22.0 * k, Stroke::new(2.5, Color32::from_white_alpha((200.0 * (1.0 - k)) as u8)));
        }
        let at = |x: f32, y: f32| self.pointer + vec2(x, y) * 1.25;
        let edge = Stroke::new(1.5, Color32::from_black_alpha(220));
        painter.add(egui::Shape::convex_polygon(vec![at(0.0, 0.0), at(0.0, 16.5), at(4.2, 12.6), at(11.8, 12.0)], Color32::WHITE, edge));
        painter.add(egui::Shape::convex_polygon(vec![at(3.4, 12.2), at(6.6, 19.6), at(9.4, 18.4), at(6.4, 11.4)], Color32::WHITE, edge));
        ctx.request_repaint();
    }

    /// A frame to ffmpeg, as many times as keeps the film on the clock: the app's time is the film's.
    fn write(&mut self, image: &egui::ColorImage, now: f64) {
        if self.ffmpeg.is_none() {
            (self.start, self.lead, self.first) = (now, self.born.elapsed().as_secs_f32(), std::time::Instant::now());
            let size = format!("{}x{}", image.size[0], image.size[1]);
            let args = [
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &size,
                "-r",
                "30",
                "-i",
                "-",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "12",
                "-pix_fmt",
                "yuv420p",
            ];
            match Command::new("ffmpeg").args(args).arg(&self.out).stdin(Stdio::piped()).spawn() {
                Ok(child) => self.ffmpeg = Some(child),
                Err(e) => eprintln!("velo: reel: ffmpeg didn't start: {e}"),
            }
        }
        let due = ((now - self.start) * FPS) as u64 + 1;
        if let Some(pipe) = self.ffmpeg.as_mut().and_then(|f| f.stdin.as_mut()) {
            while self.written < due {
                if pipe.write_all(image.as_raw()).is_err() {
                    break;
                }
                self.written += 1;
            }
        }
    }

    /// The film closed: ffmpeg writes what it has and ends. And the test loop the app heard
    /// (`VELO_AUDIO=synth`) beside it, 48 kHz mono floats from where it was at the first frame,
    /// for its sound: looped, it's in time with the picture.
    pub fn finish(&mut self) {
        if let Some(mut ffmpeg) = self.ffmpeg.take() {
            drop(ffmpeg.stdin.take());
            let _ = ffmpeg.wait();
            let take = velo_audio::synth::techno(48_000.0, 128.0, 7.5, 7);
            let from = (self.lead * 48_000.0) as usize % take.samples.len().max(1);
            let bytes: Vec<u8> = take.samples[from..].iter().chain(&take.samples[..from]).flat_map(|s| s.to_le_bytes()).collect();
            let _ = std::fs::write(self.out.with_extension("f32"), bytes);
            let _ = std::fs::write(self.out.with_extension("vf"), self.filter());
            // an audio file was the input (`VELO_AUDIO=song.mp3`): where in it the first frame
            // was, in seconds, to lay the file itself under the film from there (under 0: the
            // file started that long after the film, having taken a moment to decode)
            if let Some((started, from)) = velo_audio::track::STARTED.lock().ok().and_then(|s| *s) {
                let at = from + self.first.duration_since(started).as_secs_f64() - started.duration_since(self.first).as_secs_f64();
                let _ = std::fs::write(self.out.with_extension("lead"), format!("{at:.3}"));
            }
            eprintln!("velo: reel: {} frames in {}", self.written, self.out.display());
        }
    }
}
