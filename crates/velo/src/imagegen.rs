//! Pictures for AI Image nodes (docs/local-ai-plan.md): a model on this computer behind an
//! OpenAI-compatible images endpoint (stable-diffusion.cpp's `sd-server`, LocalAI, Ollama where
//! it has one), asked on a worker thread. Each picture is written to a file named by what's in
//! it, with what made it beside it, and added to its node with `node.add_media`: the engine only
//! ever plays files, and nothing here is needed for the next frame.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::kit::{self, Variant};
use crate::theme::Theme;
use crate::{App, ai, icons};

/// Where the pictures come from and how they're made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// An OpenAI-compatible endpoint, up to `/v1`.
    pub endpoint: String,
    /// The model the endpoint runs, empty for its own default.
    pub model: String,
    /// A side of the square picture, in pixels.
    pub size: u32,
    /// Denoising steps (servers that don't take them ignore them).
    pub steps: u32,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { endpoint: PRESETS[0].1.into(), model: String::new(), size: 512, steps: 8 }
    }
}

/// Servers on this computer that make pictures behind OpenAI's images API: name, endpoint.
/// stable-diffusion.cpp's `sd-server` listens on 1234 unless told otherwise, as LM Studio does:
/// run one of them on another port (`--listen-port`).
pub const PRESETS: [(&str, &str); 3] = [("stable-diffusion.cpp", "http://127.0.0.1:1234/v1"), ("Ollama", "http://localhost:11434/v1"), ("LocalAI", "http://localhost:8080/v1")];

/// The sizes offered: what the fast models are trained at.
pub const SIZES: [u32; 3] = [512, 768, 1024];

/// The most pictures one click asks for.
pub const MOST: usize = 8;

/// The request for one picture. stable-diffusion.cpp reads what OpenAI's schema has no room
/// for (seed, steps) from a tag in the prompt; other servers take them as fields, or ignore them.
pub fn request(prefs: &Prefs, prompt: &str, seed: i64) -> Value {
    let size = format!("{0}x{0}", prefs.size);
    let sd_cpp = ai::host(&prefs.endpoint) == ai::host(PRESETS[0].1);
    let prompt = if sd_cpp { format!("{prompt}<sd_cpp_extra_args>{}</sd_cpp_extra_args>", json!({ "seed": seed, "sample_steps": prefs.steps })) } else { prompt.to_string() };
    let mut body = json!({ "prompt": prompt, "n": 1, "size": size, "response_format": "b64_json", "seed": seed, "steps": prefs.steps });
    if !prefs.model.trim().is_empty() {
        body["model"] = json!(prefs.model.trim());
    }
    body
}

/// The picture's bytes in a reply: `b64_json`, or a `url` (a data URL, or one to fetch).
pub fn picture_in(reply: &Value) -> Result<Picture, String> {
    let first = &reply["data"][0];
    if let Some(b64) = first["b64_json"].as_str() {
        return decode(b64).map(Picture::Bytes).ok_or_else(|| tr!("The picture came back unreadable.").into());
    }
    match first["url"].as_str() {
        Some(url) if url.starts_with("data:") => url.split_once(',').and_then(|(_, b64)| decode(b64)).map(Picture::Bytes).ok_or_else(|| tr!("The picture came back unreadable.").into()),
        Some(url) => Ok(Picture::Url(url.to_string())),
        None => Err(tr!("The answer has no picture in it.").into()),
    }
}

/// A picture in a reply: its bytes, or where to fetch them.
#[derive(Debug, PartialEq)]
pub enum Picture {
    Bytes(Vec<u8>),
    Url(String),
}

/// Base64 back to bytes (spaces and newlines skipped); None if it isn't base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let digits: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=').map(value).collect::<Option<_>>()?;
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &d)| n | u32::from(d) << (18 - 6 * i));
        out.extend((0..chunk.len().saturating_sub(1)).map(|i| (n >> (16 - 8 * i)) as u8));
    }
    Some(out)
}

/// Where the pictures are kept: Velo's data folder, as the AI panel's pictures are. Export
/// with Media packs the ones a project plays.
pub fn generated_dir() -> PathBuf {
    crate::files::data_dir().unwrap_or_else(std::env::temp_dir).join("generated")
}

/// Keeps a picture: checked to be one, named by what's in it (the same picture twice is one
/// file), with what made it beside it in a .json.
fn keep_in(dir: &std::path::Path, bytes: &[u8], prompt: &str, seed: i64, prefs: &Prefs) -> Result<PathBuf, String> {
    let format = image::guess_format(bytes).map_err(|_| tr!("The answer isn't a picture."))?;
    image::load_from_memory_with_format(bytes, format).map_err(|e| e.to_string())?;
    let ext = format.extensions_str().first().copied().unwrap_or("png");
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:016x}.{ext}", hash.finish()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let made = json!({ "prompt": prompt, "seed": seed, "model": prefs.model, "size": prefs.size, "steps": prefs.steps });
    let _ = std::fs::write(path.with_extension("json"), serde_json::to_string_pretty(&made).unwrap_or_default());
    Ok(path)
}

/// Asks the endpoint for one picture and keeps it.
fn make(prefs: &Prefs, prompt: &str, seed: i64) -> Result<PathBuf, String> {
    make_in(&generated_dir(), prefs, prompt, seed)
}

fn make_in(dir: &std::path::Path, prefs: &Prefs, prompt: &str, seed: i64) -> Result<PathBuf, String> {
    let url = format!("{}/images/generations", prefs.endpoint.trim().trim_end_matches('/'));
    let mut asking = ai::http().post(&url).header("Content-Type", "application/json");
    if let Some(key) = ai::key(&prefs.endpoint) {
        asking = asking.header("Authorization", format!("Bearer {key}"));
    }
    let reply = asking.send(request(prefs, prompt, seed).to_string()).map_err(|e| ai::unreached(&prefs.endpoint, e)).and_then(ai::read)?;
    let bytes = match picture_in(&reply)? {
        Picture::Bytes(bytes) => bytes,
        Picture::Url(url) => {
            let mut fetched = ai::http().get(&url).call().map_err(|e| ai::unreached(&url, e))?;
            fetched.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| e.to_string())?
        }
    };
    keep_in(dir, &bytes, prompt, seed, prefs)
}

/// A seed for each picture: the ones asked for in a row differ.
fn seed() -> i64 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (nanos % (1 << 31)) as i64
}

/// Pictures being made for one node, one after another on a thread of their own.
pub struct Job {
    /// Each picture as it's kept, or why it wasn't; the channel closes when they're all done.
    pictures: Receiver<Result<PathBuf, String>>,
    stop: Arc<AtomicBool>,
    pub asked: usize,
    pub done: usize,
}

impl Job {
    pub fn start(prefs: Prefs, prompt: String, count: usize, ctx: egui::Context) -> Job {
        let (tx, pictures) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            for _ in 0..count {
                if stopped.load(Relaxed) {
                    break;
                }
                let made = make(&prefs, &prompt, seed());
                let failed = made.is_err();
                let _ = tx.send(made);
                ctx.request_repaint();
                // one that failed says why once, not count times
                if failed {
                    break;
                }
            }
        });
        Job { pictures, stop, asked: count, done: 0 }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // the picture being made finishes on its thread, and is dropped
        self.stop.store(true, Relaxed);
    }
}

/// The jobs running, by node, and what each node's last one said went wrong.
#[derive(Default)]
pub struct Gens {
    jobs: HashMap<String, Job>,
    failed: HashMap<String, String>,
    /// The endpoint's test, while it runs and once it answered.
    checking: Option<Receiver<Result<String, String>>>,
    checked: Option<Result<String, String>>,
}

impl App {
    /// Pictures that came: each added to its node, in whichever scene it is, one undo step each.
    pub fn poll_generations(&mut self) {
        let mut came = Vec::new();
        let mut finished = Vec::new();
        for (node, job) in &mut self.gens.jobs {
            loop {
                match job.pictures.try_recv() {
                    Ok(Ok(path)) => {
                        job.done += 1;
                        came.push((node.clone(), Ok(path)));
                    }
                    Ok(Err(e)) => came.push((node.clone(), Err(e))),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished.push(node.clone());
                        break;
                    }
                }
            }
        }
        for node in finished {
            self.gens.jobs.remove(&node);
        }
        for (node, made) in came {
            match made {
                Ok(path) => {
                    self.gens.failed.remove(&node);
                    // the node may have gone meanwhile: the picture stays on disk, unplayed
                    let Some(scene) = self.session.project.graphs().find(|s| s.node(&node).is_some()).map(|s| s.id.clone()) else { continue };
                    self.session.close_merge();
                    self.run("node.add_media", json!({ "node": node, "media": path.to_string_lossy(), "scene": scene }));
                }
                Err(e) => {
                    self.gens.failed.insert(node, e);
                }
            }
        }
        if let Some(rx) = &self.gens.checking
            && let Ok(answer) = rx.try_recv()
        {
            self.gens.checked = Some(answer);
            self.gens.checking = None;
        }
    }

    /// An AI Image's prompt, and the buttons that make pictures from it.
    pub fn ai_image_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, first: bool) {
        let Some(n) = self.session.active().and_then(|s| s.node(node)) else { return };
        let mut prompt = n.text.clone().unwrap_or_default();
        let running = self.gens.jobs.get(node).map(|j| (j.done, j.asked));
        let failed = self.gens.failed.get(node).cloned();
        let (mut typed, mut done, mut ask, mut stop) = (false, false, None, false);
        kit::section(ui, t, icons::SPARKLE, tr!("PROMPT"), first, |ui| {
            let field = egui::TextEdit::multiline(&mut prompt).desired_rows(3).desired_width(f32::INFINITY).font(t.body()).hint_text(tr!("What the pictures show: a subject, a style, colors, light"));
            let r = ui.add(field);
            (typed, done) = (r.changed(), r.lost_focus());
            ui.add_space(t.gap);
            match running {
                Some((made, asked)) => {
                    kit::hint(ui, t, &tr!("Making {} of {}…", made + 1, asked));
                    stop = kit::actions(ui, t, &[(Variant::Destructive, Some(icons::X), tr!("Stop"))])[0].clicked();
                }
                None => {
                    let empty = prompt.trim().is_empty();
                    ui.add_enabled_ui(!empty, |ui| {
                        let shown = kit::actions(ui, t, &[(Variant::Primary, Some(icons::SPARKLE), tr!("Generate")), (Variant::Outline, None, &tr!("Generate {}", 4))]);
                        if shown[0].clicked() {
                            ask = Some(1);
                        }
                        if shown[1].clone().on_hover_text(tr!("Four pictures, one after another")).clicked() {
                            ask = Some(4);
                        }
                    });
                    kit::hint(ui, t, &tr!("Made on this computer by {} (Preferences → AI and agents)", ai::host(&self.settings.imagegen.endpoint)));
                }
            }
            if let Some(e) = &failed {
                kit::warning(ui, t, e);
            }
        });
        if typed {
            self.run_merge("node.set_text", json!({ "node": node, "text": prompt }), &format!("text {node}"));
        }
        if done {
            self.session.close_merge();
        }
        if stop {
            self.gens.jobs.remove(node);
        }
        if let Some(count) = ask {
            self.gens.failed.remove(node);
            let job = Job::start(self.settings.imagegen.clone(), prompt.trim().to_string(), count.min(MOST), ui.ctx().clone());
            self.gens.jobs.insert(node.to_string(), job);
        }
    }

    /// Where AI Image nodes get their pictures: the endpoint, its model, the size and steps.
    pub fn prefs_images(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let mut test = false;
        kit::card(ui, t, tr!("AI IMAGE"), |ui| {
            kit::para(ui, t, tr!("AI Image nodes ask a model on this computer for their pictures: stable-diffusion.cpp's sd-server, LocalAI, or any server with OpenAI's images API."), t.fg_text);
            ui.add_space(t.gap);
            let p = &mut self.settings.imagegen;
            kit::setting(ui, t, tr!("Endpoint"), "", |ui| ui.add(kit::field(&mut p.endpoint).desired_width(240.0)));
            ui.horizontal_wrapped(|ui| {
                for (name, endpoint) in PRESETS {
                    if kit::button(ui, t, None, name, ai::host(&p.endpoint) == ai::host(endpoint)).clicked() {
                        p.endpoint = endpoint.to_string();
                    }
                }
            });
            kit::divider(ui, t);
            kit::setting(ui, t, tr!("Model"), tr!("Empty: the one the server runs."), |ui| ui.add(kit::field(&mut p.model).desired_width(240.0)));
            kit::divider(ui, t);
            kit::setting(ui, t, tr!("Size"), "", |ui| {
                let labels: Vec<String> = SIZES.iter().map(|s| format!("{s}")).collect();
                let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
                let current = SIZES.iter().position(|s| *s == p.size).unwrap_or(0);
                if let Some(k) = kit::segmented(ui, t, current, &labels) {
                    p.size = SIZES[k];
                }
            });
            kit::divider(ui, t);
            kit::setting(ui, t, tr!("Steps"), tr!("Turbo models: 1 to 8. Others: 20 or more."), |ui| ui.add(egui::DragValue::new(&mut p.steps).range(1..=80)));
            ui.add_space(t.gap);
            test = kit::button(ui, t, Some(icons::LIGHTNING), tr!("Test the connection"), false).clicked();
            match (&self.gens.checking, &self.gens.checked) {
                (Some(_), _) => {
                    kit::hint(ui, t, tr!("Asking…"));
                }
                (None, Some(Ok(said))) => kit::callout(ui, t, icons::CHECK, said, t.accent),
                (None, Some(Err(e))) => {
                    kit::warning(ui, t, e);
                }
                (None, None) => {}
            }
            ui.add_space(t.pad);
        });
        if test && self.gens.checking.is_none() {
            let (tx, rx) = channel();
            let endpoint = self.settings.imagegen.endpoint.clone();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let answer = ai::models(&endpoint).map(|m| if m.is_empty() { tr!("It answers.").to_string() } else { tr!("Connected: {} models", m.len()) });
                let _ = tx.send(answer);
                ctx.request_repaint();
            });
            self.gens.checking = Some(rx);
            self.gens.checked = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_comes_back_as_it_went() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7, 9]] {
            assert_eq!(decode(&crate::agent::base64(bytes)).as_deref(), Some(bytes));
        }
        assert_eq!(decode("Zm9v\nYmFy").as_deref(), Some(&b"foobar"[..]), "lines are skipped");
        assert_eq!(decode("Zm9v!"), None);
    }

    #[test]
    fn a_reply_gives_its_picture() {
        let png = crate::agent::base64(b"\x89PNG");
        assert_eq!(picture_in(&json!({ "data": [{ "b64_json": png }] })), Ok(Picture::Bytes(b"\x89PNG".to_vec())));
        assert_eq!(picture_in(&json!({ "data": [{ "url": format!("data:image/png;base64,{png}") }] })), Ok(Picture::Bytes(b"\x89PNG".to_vec())));
        assert_eq!(picture_in(&json!({ "data": [{ "url": "http://localhost:8080/out/1.png" }] })), Ok(Picture::Url("http://localhost:8080/out/1.png".into())));
        assert!(picture_in(&json!({ "data": [] })).is_err());
    }

    #[test]
    fn the_request_follows_openais_shape() {
        let mut prefs = Prefs { endpoint: "http://localhost:8080/v1".into(), model: " z-image ".into(), size: 768, steps: 4 };
        let body = request(&prefs, "neon jellyfish", 42);
        assert_eq!((body["prompt"].as_str(), body["size"].as_str(), body["model"].as_str()), (Some("neon jellyfish"), Some("768x768"), Some("z-image")));
        assert_eq!((body["response_format"].as_str(), body["n"].as_i64(), body["seed"].as_i64()), (Some("b64_json"), Some(1), Some(42)));
        // stable-diffusion.cpp's seed and steps ride in the prompt; no model, none sent
        prefs.endpoint = PRESETS[0].1.into();
        prefs.model.clear();
        let body = request(&prefs, "neon jellyfish", 42);
        let prompt = body["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("neon jellyfish<sd_cpp_extra_args>") && prompt.contains(r#""seed":42"#) && prompt.contains(r#""sample_steps":4"#), "{prompt}");
        assert!(body.get("model").is_none());
    }

    #[test]
    fn a_picture_comes_from_a_server_over_http() {
        use std::io::{BufRead, BufReader, Read, Write};
        let mut png = Vec::new();
        image::RgbImage::from_pixel(2, 2, image::Rgb([10, 200, 90])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let reply = json!({ "created": 1, "data": [{ "b64_json": crate::agent::base64(&png) }] }).to_string();
        // one request: its path and body back to the test, the picture to the client
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let (mut first, mut length) = (String::new(), 0);
            reader.read_line(&mut first).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if let Some(n) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = n.trim().parse().unwrap();
                }
                if line.trim().is_empty() {
                    break;
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let answer = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            reader.get_mut().write_all(answer.as_bytes()).unwrap();
            (first, String::from_utf8(body).unwrap())
        });
        let dir = std::env::temp_dir().join(format!("velo-gen-http-{}", std::process::id()));
        let prefs = Prefs { endpoint: format!("http://127.0.0.1:{port}/v1"), model: "z-image".into(), size: 512, steps: 8 };
        let path = make_in(&dir, &prefs, "green", 3).unwrap();
        let (first, body) = server.join().unwrap();
        assert!(first.starts_with("POST /v1/images/generations "), "{first}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["prompt"], "green");
        assert_eq!(std::fs::read(&path).unwrap(), png);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_is_kept_once_with_what_made_it() {
        let dir = std::env::temp_dir().join(format!("velo-gen-{}", std::process::id()));
        let mut png = Vec::new();
        image::RgbImage::from_pixel(4, 4, image::Rgb([200, 40, 90])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let prefs = Prefs::default();
        let a = keep_in(&dir, &png, "a pink square", 7, &prefs).unwrap();
        let b = keep_in(&dir, &png, "a pink square", 7, &prefs).unwrap();
        assert_eq!(a, b, "the same picture is one file");
        assert_eq!(a.extension().and_then(|e| e.to_str()), Some("png"));
        let made: Value = serde_json::from_str(&std::fs::read_to_string(a.with_extension("json")).unwrap()).unwrap();
        assert_eq!((made["prompt"].as_str(), made["seed"].as_i64()), (Some("a pink square"), Some(7)));
        assert!(keep_in(&dir, b"not a picture", "x", 1, &prefs).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
