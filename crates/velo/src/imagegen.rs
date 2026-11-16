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
    /// A side of the square picture, in pixels: with `canvas_shape`, the picture has as many
    /// pixels (what the time to make one goes by) in the canvas' shape.
    pub size: u32,
    /// Denoising steps (servers that don't take them ignore them).
    pub steps: u32,
    /// The pictures take the canvas' shape (16:9 for 1920×1080), so they fill it uncut.
    pub canvas_shape: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { endpoint: PRESETS[0].1.into(), model: String::new(), size: 512, steps: 4, canvas_shape: true }
    }
}

/// Servers on this computer that make pictures behind OpenAI's images API: name, endpoint.
/// stable-diffusion.cpp's `sd-server` listens on 1234 unless told otherwise, as LM Studio does:
/// run one of them on another port (`--listen-port`).
pub const PRESETS: [(&str, &str); 3] = [("stable-diffusion.cpp", "http://127.0.0.1:1234/v1"), ("Ollama", "http://localhost:11434/v1"), ("LocalAI", "http://localhost:8080/v1")];

/// The sizes offered: what the fast models are trained at.
pub const SIZES: [u32; 3] = [512, 768, 1024];

/// From how many steps stable-diffusion.cpp's EasyCache pays.
const EASYCACHE_FROM: u32 = 8;

/// The most pictures one click asks for.
pub const MOST: usize = 8;

/// A picture's width and height: `size` squared, or as many pixels in the canvas' shape, both
/// multiples of 16 (what the models' latents and patches divide by), at least 64.
pub fn dims(prefs: &Prefs, canvas: [u32; 2]) -> [u32; 2] {
    let round = |x: f64| ((x / 16.0).round() as u32 * 16).max(64);
    if !prefs.canvas_shape || canvas[0] == 0 || canvas[1] == 0 {
        return [round(f64::from(prefs.size)); 2];
    }
    let area = f64::from(prefs.size).powi(2);
    let aspect = f64::from(canvas[0]) / f64::from(canvas[1]);
    let w = round((area * aspect).sqrt());
    [w, round(area / f64::from(w))]
}

/// The request for one picture. stable-diffusion.cpp reads what OpenAI's schema has no room
/// for (seed, steps) from a tag in the prompt; other servers take them as fields, or ignore them.
pub fn request(prefs: &Prefs, prompt: &str, seed: i64, [w, h]: [u32; 2]) -> Value {
    let size = format!("{w}x{h}");
    let sd_cpp = ai::host(&prefs.endpoint) == ai::host(PRESETS[0].1);
    let prompt = if sd_cpp {
        let mut extra = json!({ "seed": seed, "sample_params": { "sample_steps": prefs.steps } });
        // reusing a step's output where the next changes little: a quarter off at 8 steps and
        // more, the same picture to the eye; at 4 or 6 it costs more than it saves (measured,
        // docs/local-ai-plan.md §3.1)
        if prefs.steps >= EASYCACHE_FROM {
            extra["cache_mode"] = json!("easycache");
        }
        format!("{prompt}<sd_cpp_extra_args>{extra}</sd_cpp_extra_args>")
    } else {
        prompt.to_string()
    };
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
fn keep_in(dir: &std::path::Path, bytes: &[u8], prompt: &str, seed: i64, prefs: &Prefs, [w, h]: [u32; 2]) -> Result<PathBuf, String> {
    let format = image::guess_format(bytes).map_err(|_| tr!("The answer isn't a picture."))?;
    image::load_from_memory_with_format(bytes, format).map_err(|e| e.to_string())?;
    let ext = format.extensions_str().first().copied().unwrap_or("png");
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hash);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:016x}.{ext}", hash.finish()));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let made = json!({ "prompt": prompt, "seed": seed, "model": prefs.model, "width": w, "height": h, "steps": prefs.steps });
    let _ = std::fs::write(path.with_extension("json"), serde_json::to_string_pretty(&made).unwrap_or_default());
    Ok(path)
}

/// Asks the endpoint for one picture and keeps it.
fn make(prefs: &Prefs, prompt: &str, seed: i64, dims: [u32; 2]) -> Result<PathBuf, String> {
    make_in(&generated_dir(), prefs, prompt, seed, dims)
}

fn make_in(dir: &std::path::Path, prefs: &Prefs, prompt: &str, seed: i64, dims: [u32; 2]) -> Result<PathBuf, String> {
    let url = format!("{}/images/generations", prefs.endpoint.trim().trim_end_matches('/'));
    let mut asking = ai::http().post(&url).header("Content-Type", "application/json");
    if let Some(key) = ai::key(&prefs.endpoint) {
        asking = asking.header("Authorization", format!("Bearer {key}"));
    }
    let reply = asking.send(request(prefs, prompt, seed, dims).to_string()).map_err(|e| ai::unreached(&prefs.endpoint, e)).and_then(ai::read)?;
    let bytes = match picture_in(&reply)? {
        Picture::Bytes(bytes) => bytes,
        Picture::Url(url) => {
            let mut fetched = ai::http().get(&url).call().map_err(|e| ai::unreached(&url, e))?;
            fetched.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| e.to_string())?
        }
    };
    keep_in(dir, &bytes, prompt, seed, prefs, dims)
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
    pub fn start(prefs: Prefs, prompt: String, count: usize, dims: [u32; 2], ctx: Option<egui::Context>) -> Job {
        let (tx, pictures) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        std::thread::spawn(move || {
            for _ in 0..count {
                if stopped.load(Relaxed) {
                    break;
                }
                let made = make(&prefs, &prompt, seed(), dims);
                let failed = made.is_err();
                let _ = tx.send(made);
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
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
    /// Pictures made for a node of a scene on the output, held until it leaves: a file added to
    /// a list rebuilds its scene. The list itself carries its files over (`Reel::adopt`), but
    /// the rest starts over (movies from their start, Feedback's trails from black), which the
    /// audience shouldn't see unasked.
    waiting: HashMap<String, Vec<PathBuf>>,
    /// The scenes on the output last time it was looked at: one that leaves has its AI Images
    /// with Refresh set to "Off the output" make new pictures.
    on_output: Option<std::collections::HashSet<String>>,
    /// The window's context, for workers to wake it when a picture comes.
    ctx: Option<egui::Context>,
    /// The endpoint's test, while it runs and once it answered.
    checking: Option<Receiver<Result<String, String>>>,
    checked: Option<Result<String, String>>,
}

#[cfg(test)]
impl Gens {
    /// What went wrong with a node's last pictures, as its inspector says it.
    pub fn failed(&self, node: &str) -> Option<&str> {
        self.failed.get(node).map(String::as_str)
    }
}

impl App {
    /// The scene a node is in, and whether it's on the output: the live one, or a layer (on
    /// or off: a layer can come on at any time).
    pub(crate) fn node_scene(&self, node: &str) -> Option<(String, bool)> {
        let scene = self.session.project.graphs().find(|s| s.node(node).is_some())?;
        Some((scene.id.clone(), self.on_output(&scene.id)))
    }

    /// Whether a scene is on the output: the live one, or a layer (on or off: a layer can come
    /// on at any time).
    fn on_output(&self, scene: &str) -> bool {
        let live = self.live.as_ref().and_then(|l| l.live_scene()) == Some(scene);
        live || self.session.project.scenes.iter().any(|s| s.id == scene && s.layer != velo_core::Layer::Scene)
    }

    /// Starts making `count` pictures (1 to [`MOST`]) for an AI Image from its prompt, in the
    /// canvas' shape if asked: the inspector's Generate, an agent's generate_image. How many it
    /// asked for, or why it can't.
    pub fn generate(&mut self, node: &str, count: usize) -> Result<usize, String> {
        let n = self.session.project.graphs().flat_map(|s| &s.nodes).find(|n| n.id == node).ok_or_else(|| format!("no node {node}"))?;
        if n.kind != velo_core::AI_IMAGE {
            return Err(format!("{node} isn't an AI Image node"));
        }
        let prompt = n.text.as_deref().unwrap_or_default().trim().to_string();
        if prompt.is_empty() {
            return Err(format!("{node} has no prompt yet: node.set_text gives it one"));
        }
        if self.gens.jobs.contains_key(node) {
            return Err(format!("{node} is making pictures already: generation_status says how far"));
        }
        let count = count.clamp(1, MOST);
        let canvas = &self.session.project.canvas;
        let dims = dims(&self.settings.imagegen, [canvas.width, canvas.height]);
        self.gens.failed.remove(node);
        let job = Job::start(self.settings.imagegen.clone(), prompt, count, dims, self.gens.ctx.clone());
        self.gens.jobs.insert(node.to_string(), job);
        Ok(count)
    }

    /// How a node's pictures are coming (every AI Image's, without `node`): being made, held
    /// for a scene on the output, in the node, or why the last ones failed.
    pub fn generation_status(&self, node: Option<&str>) -> serde_json::Value {
        let nodes: Vec<&velo_core::Node> = self.session.project.graphs().flat_map(|s| &s.nodes).filter(|n| n.kind == velo_core::AI_IMAGE && node.is_none_or(|id| n.id == id)).collect();
        let each = nodes.iter().map(|n| {
            let making = self.gens.jobs.get(&n.id).map(|j| json!({ "done": j.done, "asked": j.asked }));
            json!({
                "node": n.id,
                "prompt": n.text,
                "making": making,
                "held_for_the_output": self.gens.waiting.get(&n.id).map_or(0, Vec::len),
                "pictures": velo_core::media_list(n).len(),
                "error": self.gens.failed.get(&n.id),
            })
        });
        json!({ "ai_images": each.collect::<Vec<_>>(), "server": self.settings.imagegen.endpoint })
    }

    /// Adds pictures to a node, one undo step each.
    fn add_pictures(&mut self, node: &str, scene: &str, pictures: &[PathBuf]) {
        for path in pictures {
            self.session.close_merge();
            self.run("node.add_media", json!({ "node": node, "media": path.to_string_lossy(), "scene": scene }));
        }
    }

    /// Pictures that came: each added to its node, in whichever scene it is, one undo step each;
    /// held while that scene is on the output, and added once it's off.
    pub fn poll_generations(&mut self, ctx: &egui::Context) {
        if self.gens.ctx.is_none() {
            self.gens.ctx = Some(ctx.clone());
        }
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
                    match self.node_scene(&node) {
                        Some((_, true)) => self.gens.waiting.entry(node).or_default().push(path),
                        Some((scene, false)) => self.add_pictures(&node, &scene, &[path]),
                        None => {}
                    }
                }
                Err(e) => {
                    self.gens.failed.insert(node, e);
                }
            }
        }
        // scenes that left the output: new pictures for the AI Images that refresh then
        let now: std::collections::HashSet<String> = self.session.project.scenes.iter().filter(|s| self.on_output(&s.id)).map(|s| s.id.clone()).collect();
        if let Some(before) = self.gens.on_output.replace(now.clone()) {
            let left: Vec<String> = before.difference(&now).cloned().collect();
            for scene in left {
                let refreshing: Vec<(String, usize)> = self
                    .session
                    .project
                    .scenes
                    .iter()
                    .filter(|s| s.id == scene)
                    .flat_map(|s| &s.nodes)
                    .filter(|n| n.kind == velo_core::AI_IMAGE && !n.off && n.params.get("refresh").is_some_and(|v| v[0].round() == 1.0))
                    .map(|n| (n.id.clone(), velo_core::list_room(n)))
                    .collect();
                for (node, pool) in refreshing {
                    // a node without a prompt, or busy already, just keeps what it has
                    let _ = self.generate(&node, pool.min(MOST));
                }
            }
        }
        // held pictures whose scene went off the output
        let off: Vec<String> = self.gens.waiting.keys().filter(|n| self.node_scene(n).is_none_or(|(_, on)| !on)).cloned().collect();
        for node in off {
            let pictures = self.gens.waiting.remove(&node).unwrap_or_default();
            if let Some((scene, _)) = self.node_scene(&node) {
                self.add_pictures(&node, &scene, &pictures);
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
        let held = self.gens.waiting.get(node).map_or(0, Vec::len);
        let (mut typed, mut done, mut ask, mut stop, mut now) = (false, false, None, false, false);
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
            if held > 0 {
                ui.add_space(t.gap);
                kit::hint(ui, t, &tr!("{} new, held while this scene is on the output: adding them rebuilds it, and its movies and trails start over.", held));
                now = kit::actions(ui, t, &[(Variant::Outline, Some(icons::PLUS), tr!("Add them now"))])[0].clicked();
            }
            if let Some(e) = &failed {
                kit::warning(ui, t, e);
            }
        });
        if now && let Some((scene, _)) = self.node_scene(node) {
            let pictures = self.gens.waiting.remove(node).unwrap_or_default();
            self.add_pictures(node, &scene, &pictures);
        }
        if typed {
            self.run_merge("node.set_text", json!({ "node": node, "text": prompt }), &format!("text {node}"));
        }
        if done {
            self.session.close_merge();
        }
        if stop {
            self.gens.jobs.remove(node);
        }
        if let Some(count) = ask
            && let Err(e) = self.generate(node, count)
        {
            self.gens.failed.insert(node.to_string(), e);
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
            kit::setting(ui, t, tr!("The canvas' shape"), tr!("As many pixels in the canvas' shape: they fill it uncut, in the same time."), |ui| kit::switch(ui, t, &mut p.canvas_shape, tr!("The canvas' shape")));
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
    fn pictures_take_the_canvas_shape_at_the_same_cost() {
        let mut prefs = Prefs::default();
        assert_eq!(dims(&prefs, [1920, 1080]), [688, 384], "16:9, about 512 squared");
        assert_eq!(dims(&prefs, [1080, 1920]), [384, 688], "upright");
        assert_eq!(dims(&prefs, [1024, 1024]), [512, 512]);
        for canvas in [[1920, 1080], [1280, 720], [3840, 1080], [1080, 1350], [800, 600]] {
            let [w, h] = dims(&prefs, canvas);
            assert!(w % 16 == 0 && h % 16 == 0, "{canvas:?}: {w}x{h} by 16");
            let pixels = f64::from(w * h) / 512f64.powi(2);
            assert!((0.9..1.1).contains(&pixels), "{canvas:?}: {w}x{h} is {pixels:.2} of 512 squared");
            let off = (f64::from(w) / f64::from(h)) / (f64::from(canvas[0]) / f64::from(canvas[1]));
            assert!((0.95..1.05).contains(&off), "{canvas:?}: {w}x{h} keeps its shape ({off:.3})");
        }
        prefs.canvas_shape = false;
        assert_eq!(dims(&prefs, [1920, 1080]), [512, 512], "square when asked");
        assert_eq!(dims(&Prefs { size: 10, ..prefs }, [0, 0]), [64, 64], "never under 64");
    }

    #[test]
    fn the_request_follows_openais_shape() {
        let mut prefs = Prefs { endpoint: "http://localhost:8080/v1".into(), model: " z-image ".into(), size: 768, steps: 4, canvas_shape: false };
        let body = request(&prefs, "neon jellyfish", 42, [768, 768]);
        assert_eq!((body["prompt"].as_str(), body["size"].as_str(), body["model"].as_str()), (Some("neon jellyfish"), Some("768x768"), Some("z-image")));
        assert_eq!((body["response_format"].as_str(), body["n"].as_i64(), body["seed"].as_i64()), (Some("b64_json"), Some(1), Some(42)));
        // stable-diffusion.cpp's seed and steps ride in the prompt; no model, none sent
        prefs.endpoint = PRESETS[0].1.into();
        prefs.model.clear();
        let body = request(&prefs, "neon jellyfish", 42, [688, 384]);
        assert_eq!(body["size"].as_str(), Some("688x384"));
        let prompt = body["prompt"].as_str().unwrap();
        assert!(prompt.starts_with("neon jellyfish<sd_cpp_extra_args>") && prompt.contains(r#""seed":42"#) && prompt.contains(r#""sample_params":{"sample_steps":4}"#), "{prompt}");
        assert!(body.get("model").is_none());
        assert!(!prompt.contains("cache_mode"), "4 steps: no cache");
        prefs.steps = 8;
        let body = request(&prefs, "neon jellyfish", 42, [688, 384]);
        assert!(body["prompt"].as_str().unwrap().contains(r#""cache_mode":"easycache""#), "8 steps: EasyCache");
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
        let prefs = Prefs { endpoint: format!("http://127.0.0.1:{port}/v1"), model: "z-image".into(), size: 512, steps: 8, canvas_shape: false };
        let path = make_in(&dir, &prefs, "green", 3, [512, 512]).unwrap();
        let (first, body) = server.join().unwrap();
        assert!(first.starts_with("POST /v1/images/generations "), "{first}");
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["prompt"], "green");
        assert_eq!(std::fs::read(&path).unwrap(), png);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pictures_for_a_scene_on_the_output_wait_until_it_leaves() {
        let mut app = App::for_tests();
        app.run("scene.add", json!({}));
        app.run("node.add", json!({ "kind": velo_core::AI_IMAGE }));
        let node = app.session.active().unwrap().nodes[0].id.clone();
        let files = |app: &App| velo_core::media_list(app.session.active().unwrap().node(&node).unwrap()).into_iter().map(String::from).collect::<Vec<_>>();
        // a job whose pictures this test hands over
        let came = |app: &mut App, path: &str| {
            let (tx, pictures) = channel();
            tx.send(Ok(PathBuf::from(path))).unwrap();
            // the job's thread is done: its channel closes
            drop(tx);
            app.gens.jobs.insert(node.clone(), Job { pictures, stop: Arc::new(AtomicBool::new(false)), asked: 1, done: 0 });
            app.poll_generations(&egui::Context::default());
        };
        came(&mut app, "/gen/a.png");
        assert_eq!(files(&app), ["/gen/a.png"], "off the output: added as it comes");
        // a layer can come on at any time: held
        app.run("scene.set_layer", json!({ "layer": "Top" }));
        came(&mut app, "/gen/b.png");
        assert_eq!(files(&app), ["/gen/a.png"]);
        assert_eq!(app.gens.waiting[&node], [PathBuf::from("/gen/b.png")]);
        assert!(app.gens.jobs.is_empty(), "its job done");
        // a scene again: what waited goes in
        app.run("scene.set_layer", json!({ "layer": "Scene" }));
        app.poll_generations(&egui::Context::default());
        assert_eq!(files(&app), ["/gen/a.png", "/gen/b.png"]);
        assert!(app.gens.waiting.is_empty());
    }

    #[test]
    fn an_agent_asks_for_pictures_without_waiting_for_them() {
        let mut app = App::for_tests();
        // nothing listens on the discard port: the job fails at once
        app.settings.imagegen.endpoint = "http://127.0.0.1:9/v1".into();
        app.run("scene.add", json!({}));
        app.run("node.add", json!({ "kind": velo_core::AI_IMAGE }));
        app.run("node.add", json!({ "kind": "velo.noise" }));
        let (ai, noise) = {
            let nodes = &app.session.active().unwrap().nodes;
            (nodes[0].id.clone(), nodes[1].id.clone())
        };
        app.settings.agent.edit = false;
        assert!(app.agent_call("generate_image", &json!({ "node": ai, "prompt": "a lantern" }), false).is_err(), "a read-only agent can't");
        app.settings.agent.edit = true;
        let refused = |app: &mut App, p: Value| app.agent_call("generate_image", &p, false).unwrap_err();
        assert!(refused(&mut app, json!({ "node": ai })).contains("no prompt"), "no prompt, nothing to make");
        assert!(refused(&mut app, json!({ "node": noise, "prompt": "x" })).contains("isn't an AI Image"));
        assert!(refused(&mut app, json!({ "node": "n_nope" })).contains("no node"));
        let answer = app.agent_call("generate_image", &json!({ "node": ai, "prompt": "a red lantern in fog", "count": 20 }), false).unwrap();
        assert_eq!(answer["making"], 8, "at most eight at a time");
        assert_eq!(app.session.active().unwrap().nodes[0].text.as_deref(), Some("a red lantern in fog"), "the prompt set");
        assert!(refused(&mut app, json!({ "node": ai })).contains("already"), "one job a node");
        let status = app.agent_call("generation_status", &json!({ "node": ai }), false).unwrap();
        assert_eq!(status["ai_images"][0]["making"]["asked"], 8);
        // the job fails on its thread; the status says why
        for _ in 0..500 {
            app.poll_generations(&egui::Context::default());
            if app.gens.jobs.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let status = app.agent_call("generation_status", &json!({}), false).unwrap();
        let each = &status["ai_images"][0];
        assert!(each["making"].is_null() && each["pictures"] == 0, "{each}");
        assert!(each["error"].as_str().is_some_and(|e| e.contains("Nothing answers")), "{each}");
        // the tools are offered to every agent
        let tools = velo_mcp::tools();
        for name in ["generate_image", "generation_status"] {
            assert!(tools.as_array().unwrap().iter().any(|t| t["name"] == name), "{name}");
        }
    }

    #[test]
    fn an_ai_image_set_to_refresh_makes_new_pictures_when_its_scene_leaves_the_output() {
        let mut app = App::for_tests();
        app.settings.imagegen.endpoint = "http://127.0.0.1:9/v1".into();
        let ctx = egui::Context::default();
        app.run("scene.add", json!({}));
        for (prompt, refresh) in [("a lantern", 1.0), ("a moth", 0.0)] {
            app.run("node.add", json!({ "kind": velo_core::AI_IMAGE }));
            let node = app.session.active().unwrap().nodes.last().unwrap().id.clone();
            app.run("node.set_text", json!({ "node": node, "text": prompt }));
            app.run("node.set_param", json!({ "node": node, "param": "refresh", "value": refresh }));
            app.run("node.set_param", json!({ "node": node, "param": "pool", "value": 3.0 }));
        }
        let (lantern, moth) = {
            let n = &app.session.active().unwrap().nodes;
            (n[0].id.clone(), n[1].id.clone())
        };
        app.poll_generations(&ctx);
        assert!(app.gens.jobs.is_empty(), "nothing left the output yet");
        // on the output (a layer), then off it
        app.run("scene.set_layer", json!({ "layer": "Top" }));
        app.poll_generations(&ctx);
        assert!(app.gens.jobs.is_empty(), "coming on makes nothing");
        app.run("scene.set_layer", json!({ "layer": "Scene" }));
        app.poll_generations(&ctx);
        assert_eq!(app.gens.jobs.get(&lantern).map(|j| j.asked), Some(3), "the one set to refresh: its pool anew");
        assert!(!app.gens.jobs.contains_key(&moth), "the other one waits to be asked");
    }

    /// Against a real server: `VELO_IMAGEGEN_ENDPOINT=http://127.0.0.1:1234/v1 cargo test -p velo
    /// a_real_server -- --ignored --nocapture` (optionally `VELO_IMAGEGEN_SIZE`, `_STEPS`).
    #[test]
    #[ignore = "needs a local images server"]
    fn a_real_server_makes_a_picture() {
        let endpoint = std::env::var("VELO_IMAGEGEN_ENDPOINT").expect("VELO_IMAGEGEN_ENDPOINT");
        let number = |name: &str, or: u32| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(or);
        let prefs = Prefs { endpoint, model: String::new(), size: number("VELO_IMAGEGEN_SIZE", 512), steps: number("VELO_IMAGEGEN_STEPS", 4), canvas_shape: true };
        let wanted = dims(&prefs, [1920, 1080]);
        let dir = std::env::temp_dir().join(format!("velo-gen-real-{}", std::process::id()));
        let started = std::time::Instant::now();
        let path = make_in(&dir, &prefs, "neon jellyfish drifting in dark smoke, glowing cyan and magenta", 42, wanted).unwrap_or_else(|e| panic!("{e}"));
        let picture = image::open(&path).unwrap();
        println!("{}x{} in {:.2} s: {}", picture.width(), picture.height(), started.elapsed().as_secs_f64(), path.display());
        assert_eq!([picture.width(), picture.height()], wanted, "the canvas' shape");
    }

    #[test]
    fn a_picture_is_kept_once_with_what_made_it() {
        let dir = std::env::temp_dir().join(format!("velo-gen-{}", std::process::id()));
        let mut png = Vec::new();
        image::RgbImage::from_pixel(4, 4, image::Rgb([200, 40, 90])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let prefs = Prefs::default();
        let a = keep_in(&dir, &png, "a pink square", 7, &prefs, [4, 4]).unwrap();
        let b = keep_in(&dir, &png, "a pink square", 7, &prefs, [4, 4]).unwrap();
        assert_eq!(a, b, "the same picture is one file");
        assert_eq!(a.extension().and_then(|e| e.to_str()), Some("png"));
        let made: Value = serde_json::from_str(&std::fs::read_to_string(a.with_extension("json")).unwrap()).unwrap();
        assert_eq!((made["prompt"].as_str(), made["seed"].as_i64()), (Some("a pink square"), Some(7)));
        assert!(keep_in(&dir, b"not a picture", "x", 1, &prefs, [4, 4]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
