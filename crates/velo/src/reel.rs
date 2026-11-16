//! `VELO_REEL=out.mp4`, in a capture run (`VELO_CAPTURE_TO`, which ends it at `VELO_CAPTURE_AT`):
//! a recording of Velo being used. A script plays the pointer and the keyboard and writes
//! captions; the window's frames go to ffmpeg, 30 a second. No screen-recording permission, and
//! nobody at the mouse: the same film every time, to make again when the interface changes.
//!
//! The script (`VELO_REEL_SCRIPT`, or `scripts/reel.txt`) is a step a line, `seconds what`:
//! `move x y` (the pointer gets there then), `click`, `down`, `up`, `key Tab`, `type words`,
//! `say words` (a caption; alone, none). Points are the window's, in points.

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Event, Key, PointerButton, Pos2, Stroke, pos2, vec2};

const FPS: f64 = 30.0;
/// How long the pointer takes to get where a `move` sends it, and a typed letter to follow the last.
const TRAVEL: f64 = 0.7;
const LETTER: f64 = 0.09;

enum Step {
    Move(Pos2),
    Click,
    Down,
    Up,
    Key(Key),
    Type(String),
    Say(String),
}

pub struct Reel {
    out: std::path::PathBuf,
    script: Vec<(f64, Step)>,
    next: usize,
    pointer: Pos2,
    clicked: f64,
    caption: String,
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
                "down" => script.push((at, Step::Down)),
                "up" => script.push((at, Step::Up)),
                "key" => script.extend(Key::from_name(rest).map(|key| (at, Step::Key(key)))),
                "type" => script.extend(rest.chars().enumerate().map(|(i, c)| (at + i as f64 * LETTER, Step::Type(c.to_string())))),
                "say" => script.push((at, Step::Say(rest.to_string()))),
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
            shot: Arc::default(),
            asked: false,
            ffmpeg: None,
            start: 0.0,
            written: 0,
            born: std::time::Instant::now(),
            lead: 0.0,
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
                Step::Down => {
                    raw.events.push(button(true));
                    self.clicked = now;
                }
                Step::Up => raw.events.push(button(false)),
                Step::Key(key) => raw.events.extend([true, false].map(|pressed| Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: Default::default() })),
                Step::Type(letters) => raw.events.push(Event::Text(letters.clone())),
                Step::Say(words) => self.caption.clone_from(words),
            }
            self.next += 1;
        }
    }

    /// Each frame: the one asked for last goes out, the next is asked for; the pointer and the
    /// caption are drawn over everything, so they're in it.
    pub fn frame(&mut self, ctx: &egui::Context) {
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
        if !self.caption.is_empty() {
            let screen = ctx.content_rect();
            let font = egui::FontId::proportional(22.0);
            let galley = painter.layout_no_wrap(self.caption.clone(), font, Color32::WHITE);
            // over the canvas' top, where nothing else is
            let pill = egui::Rect::from_center_size(pos2(screen.center().x - 20.0, screen.top() + 96.0), galley.size() + vec2(44.0, 26.0));
            painter.rect_filled(pill, 14.0, Color32::from_rgba_unmultiplied(8, 10, 12, 235));
            painter.rect_stroke(pill, 14.0, Stroke::new(1.0, Color32::from_white_alpha(40)), egui::StrokeKind::Inside);
            painter.galley(pill.center() - galley.size() / 2.0, galley, Color32::WHITE);
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
            (self.start, self.lead) = (now, self.born.elapsed().as_secs_f32());
            let size = format!("{}x{}", image.size[0], image.size[1]);
            let args =
                ["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &size, "-r", "30", "-i", "-", "-vf", "scale=1920:-2:flags=lanczos", "-c:v", "libx264", "-preset", "veryfast", "-crf", "16", "-pix_fmt", "yuv420p"];
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
            eprintln!("velo: reel: {} frames in {}", self.written, self.out.display());
        }
    }
}
