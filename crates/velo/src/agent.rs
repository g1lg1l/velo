//! Agents (M5, from Ravune's agent.rs): Velo answers MCP clients (Claude Code, Codex, any MCP
//! client) through `velo-mcp`, a small stdio bridge that talks to this JSON-lines server on
//! localhost. Off until turned on in Preferences → AI and agents. Agents always read; the user decides
//! whether they may edit, touch the live scene, or save. What an agent changes until its next
//! checkpoint undoes in one step, and every call lands in a log the user can read.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use velo_core::{NODE_KINDS, OPERATORS, SOURCES, is_shader};
use velo_engine::{BUILTINS, GLOBALS, PREVIEW, ShaderNode};

use crate::shell::Tone;
use crate::theme::Theme;
use crate::{App, icons, kit, live};

pub const PORT: u16 = 7318;
/// What `velo-mcp` reads to find and open the server: `{ "port", "token" }`.
pub const CONNECTION_FILE: &str = "agent.json";
const LOG_LINES: usize = 200;
/// Every agent edit until a checkpoint merges under this key: one undo step.
const MERGE: &str = "agent";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub enabled: bool,
    pub port: u16,
    /// Agents may run operators (change the project).
    pub edit: bool,
    /// Agents may change the scene that's live (by default they work on the others, which the
    /// preview shows).
    pub live: bool,
    /// Agents may save the project.
    pub files: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { enabled: false, port: PORT, edit: true, live: false, files: false }
    }
}

/// A request from a connection, answered on the UI thread.
struct Call {
    method: String,
    params: Value,
    /// From Velo's own AI panel: the user asked, in Velo, so it may change the live scene.
    panel: bool,
    reply: Sender<Result<Value, String>>,
}

pub struct Line {
    /// Seconds since Velo started.
    pub at: f64,
    pub what: String,
    pub ok: bool,
}

#[derive(Default)]
pub struct Agent {
    server: Option<Server>,
    pub log: Vec<Line>,
    /// Connections last frame (a drop to none closes the agent's undo step).
    clients: usize,
}

struct Server {
    port: u16,
    calls: Receiver<Call>,
    stop: Arc<AtomicBool>,
    clients: Arc<AtomicUsize>,
    file: Option<PathBuf>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(file) = &self.file {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// A secret for the connection file, so only someone who can read the user's own files can
/// talk to Velo (a web page posting to localhost can't).
// ponytail: std's randomly keyed hasher plus the clock, not a CSPRNG; fine for a local secret
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    (0..2u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(nanos ^ (i as u128) << 64);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn listen(listener: TcpListener, token: String, calls: Sender<Call>, stop: Arc<AtomicBool>, clients: Arc<AtomicUsize>, ctx: egui::Context) {
    let _ = listener.set_nonblocking(true);
    while !stop.load(Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let (token, calls, stop, clients, ctx) = (token.clone(), calls.clone(), stop.clone(), clients.clone(), ctx.clone());
                std::thread::spawn(move || serve(stream, &token, calls, stop, clients, ctx));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn send(stream: &mut TcpStream, id: &Value, result: Result<Value, String>) -> std::io::Result<()> {
    let line = match result {
        Ok(v) => json!({ "id": id, "result": v }),
        Err(e) => json!({ "id": id, "error": e }),
    };
    writeln!(stream, "{line}")
}

/// One connection: a hello carrying the token first (anything else is dropped), then one
/// JSON request per line, each answered by the UI thread.
fn serve(stream: TcpStream, token: &str, calls: Sender<Call>, stop: Arc<AtomicBool>, clients: Arc<AtomicUsize>, ctx: egui::Context) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut out) = stream.try_clone() else { return };
    let mut lines = BufReader::new(stream).lines();
    let Some(Ok(first)) = lines.next() else { return };
    let hello: Value = serde_json::from_str(&first).unwrap_or_default();
    if hello["method"] != "hello" || hello["params"]["token"].as_str() != Some(token) {
        let _ = send(&mut out, &hello["id"], Err("wrong or missing token: use velo-mcp, which reads it from Velo's data folder".into()));
        return;
    }
    if send(&mut out, &hello["id"], Ok(json!({ "name": "Velo", "version": env!("CARGO_PKG_VERSION") }))).is_err() {
        return;
    }
    let panel = hello["params"]["panel"].as_bool().unwrap_or(false);
    clients.fetch_add(1, Relaxed);
    ctx.request_repaint();
    for line in lines {
        let Ok(line) = line else { break };
        if stop.load(Relaxed) {
            break;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ = send(&mut out, &Value::Null, Err(format!("not JSON: {e}")));
                continue;
            }
        };
        let result = if request["method"] == "video_frames" {
            // decoding takes a while and needs nothing of the app's: here, off the UI thread
            video_frames(&request["params"])
        } else {
            let (reply, answer) = channel();
            let call = Call { method: request["method"].as_str().unwrap_or("").to_string(), params: request["params"].clone(), panel, reply };
            if calls.send(call).is_err() {
                break;
            }
            ctx.request_repaint();
            answer.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| Err("Velo didn't answer in time".into()))
        };
        if send(&mut out, &request["id"], result).is_err() {
            break;
        }
    }
    clients.fetch_sub(1, Relaxed);
    ctx.request_repaint();
}

/// A video's frames (or a GIF's, an image sequence's, a picture) for an agent to look at: at most
/// 12, 768 px a side at most, as JPEG.
fn video_frames(p: &Value) -> Result<Value, String> {
    let path = std::path::Path::new(p["path"].as_str().filter(|s| !s.is_empty()).ok_or("path: the video's file")?);
    // media only: an agent doesn't get to read any file it names
    let media = path.is_dir() || path.extension().is_some_and(|e| velo_engine::media::EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
    if !media {
        return Err(format!("not a video or picture Velo reads ({})", velo_engine::media::EXTENSIONS.join(", ")));
    }
    let times: Vec<f64> = p["times"].as_array().map(|a| a.iter().filter_map(Value::as_f64).take(12).collect()).unwrap_or_default();
    let count = p["count"].as_u64().unwrap_or(6).clamp(1, 12) as usize;
    let sampled = velo_engine::media::sample(path, &times, count)?;
    let round = |t: f64| (t * 100.0).round() / 100.0;
    let frames = sampled
        .frames
        .into_iter()
        .map(|(t, picture)| {
            let picture = image::DynamicImage::ImageRgba8(picture);
            let picture = if picture.width().max(picture.height()) > 768 { picture.resize(768, 768, image::imageops::FilterType::Triangle) } else { picture };
            let mut jpeg = Vec::new();
            picture.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 85)).map_err(|e| e.to_string())?;
            Ok(json!({ "time": round(t), "jpeg_base64": base64(&jpeg) }))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({ "path": path, "width": sampled.size[0], "height": sampled.size[1], "length": round(sampled.length), "frames": frames }))
}

/// `velo-mcp`, next to Velo: the stdio bridge MCP clients start.
pub fn bridge() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(format!("velo-mcp{}", std::env::consts::EXE_SUFFIX))
}

/// What adds Velo to Claude Code in every folder: without `--scope user` it's there only in
/// the folder the command ran in.
pub fn claude_command() -> String {
    format!("claude mcp add --scope user velo -- \"{}\"", bridge().display())
}

/// Velo as an entry of the MCP servers that Claude Desktop, Cursor and others keep in a file.
pub fn mcp_json() -> String {
    serde_json::to_string_pretty(&json!({ "mcpServers": { "velo": { "command": bridge().to_string_lossy() } } })).unwrap_or_default()
}

/// Operators that change a scene: what the live scene is kept safe from.
fn edits_a_scene(id: &str) -> bool {
    id != "scene.add" && ["node.", "edge.", "mapping.", "macro.", "variation.", "scene."].iter().any(|p| id.starts_with(p))
}

/// How a Shader node's code is written, for agents (docs/plugins.md §2).
fn shader_format() -> Value {
    json!({
        "file": "WGSL. First a /*{ JSON }*/ header: name, category (generator | effect), description, params [{ id, type: float|int|bool|color|vec2, default, min, max }], inputs [{ id: \"src\", type: \"texture\" }] (at most one). Then fn main_image(uv: vec2f, px: vec2f) -> vec4f.",
        "available": format!("uv is 0..1 across the picture, px in pixels. Each param is a variable by its id. g.<name> holds: {}. The audio fields are 0..1. An effect reads its input with textureSample(src, smp, uv); effects also get an `amount` param (0 passes the input through).", GLOBALS.join(", ")),
        "example": velo_engine::TEMPLATE,
        "how": "node.add with kind velo.shader and code, or node.set_code on an existing one. run_operator answers whether the code compiles (naga's message with the line otherwise); the scene keeps its last good code meanwhile.",
    })
}

/// Bytes as base64: a PNG an MCP client can look at, a picture for a model.
pub fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

impl App {
    /// Keeps the server in step with the preferences and answers what came in.
    pub fn poll_agent(&mut self, ctx: &egui::Context, now: f64) {
        let prefs = self.settings.agent.clone();
        let running = self.agent.server.as_ref().map(|s| s.port);
        if !prefs.enabled && running.is_some() || prefs.enabled && running.is_some_and(|p| p != prefs.port) {
            self.agent.server = None;
            self.session.close_merge();
        }
        if prefs.enabled && self.agent.server.is_none() {
            match TcpListener::bind(("127.0.0.1", prefs.port)) {
                Ok(listener) => {
                    let token = token();
                    let file = crate::files::data_dir().map(|d| d.join(CONNECTION_FILE));
                    if let Some(f) = &file {
                        let _ = std::fs::create_dir_all(f.parent().unwrap_or(f));
                        let _ = std::fs::write(f, json!({ "port": prefs.port, "token": token }).to_string());
                    }
                    let (tx, calls) = channel();
                    let (stop, clients) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(0)));
                    let (s, c, ctx) = (stop.clone(), clients.clone(), ctx.clone());
                    match std::thread::Builder::new().name("agents".into()).spawn(move || listen(listener, token, tx, s, c, ctx)) {
                        Ok(_) => self.agent.server = Some(Server { port: prefs.port, calls, stop, clients, file }),
                        Err(e) => self.say(Tone::Error, e.to_string()),
                    }
                }
                Err(e) => {
                    self.settings.agent.enabled = false;
                    self.say(Tone::Error, tr!("Agents can't listen on port {}: {}", prefs.port, e));
                }
            }
        }
        let Some(server) = &self.agent.server else { return };
        let clients = server.clients.load(Relaxed);
        let calls: Vec<Call> = server.calls.try_iter().collect();
        if clients == 0 && self.agent.clients > 0 {
            self.session.close_merge(); // it left: its edits are done
        }
        self.agent.clients = clients;
        // the preview's pixels, kept for snapshots while someone may ask
        if let Some(live) = &mut self.live {
            live.keep_frame = clients > 0;
        }
        for call in calls {
            let result = self.agent_call(&call.method, &call.params, call.panel);
            let detail = match call.method.as_str() {
                "run_operator" => format!("run_operator {}", call.params["id"].as_str().unwrap_or("?")),
                m => m.to_string(),
            };
            let what = match &result {
                Ok(_) => detail,
                Err(e) => format!("{detail}: {e}"),
            };
            self.agent.log.push(Line { at: now, what, ok: result.is_ok() });
            if self.agent.log.len() > LOG_LINES {
                self.agent.log.remove(0);
            }
            let _ = call.reply.send(result);
        }
    }

    pub fn agents_connected(&self) -> Option<usize> {
        self.agent.server.as_ref().map(|s| s.clients.load(Relaxed))
    }

    pub(crate) fn agent_call(&mut self, method: &str, p: &Value, panel: bool) -> Result<Value, String> {
        let prefs = self.settings.agent.clone();
        let read_only = || tr!("This agent may only read: the user can allow edits in Preferences → AI and agents").to_string();
        match method {
            "search_operators" => {
                let query = p["query"].as_str().unwrap_or("").to_lowercase();
                let words: Vec<&str> = query.split_whitespace().collect();
                let found: Vec<Value> = OPERATORS
                    .iter()
                    .filter(|o| words.iter().all(|w| o.id.contains(w) || o.label.to_lowercase().contains(w) || o.description.to_lowercase().contains(w)))
                    .map(|o| json!({ "id": o.id, "label": o.label, "description": o.description, "example": serde_json::from_str::<Value>(o.example).unwrap_or(json!({})), "undoable": o.undo }))
                    .collect();
                Ok(json!({ "operators": found, "count": found.len() }))
            }
            "run_operator" => {
                if !prefs.edit {
                    return Err(read_only());
                }
                let id = p["id"].as_str().ok_or("run_operator needs the operator's id")?;
                let props = p.get("props").filter(|v| !v.is_null()).cloned().unwrap_or(json!({}));
                // the live scene is the audience's: outside agents work on the others unless
                // allowed; Velo's own AI panel may (the user is asking, in Velo)
                let target = props["scene"].as_str().map(String::from).or_else(|| self.session.active_scene.clone());
                let live = self.live.as_ref().and_then(|l| l.live_scene().map(String::from));
                if !prefs.live && !panel && edits_a_scene(id) && target.is_some() && target == live {
                    return Err(tr!("That scene is live: work on another one (scene.add makes a new one and selects it; the preview shows it), or the user allows editing live in Preferences → AI and agents").into());
                }
                self.session.run_merge(id, props.clone(), MERGE)?;
                let mut answer = json!({ "ok": true, "undo": "one Ctrl+Z takes back everything changed since the last checkpoint" });
                if id == "node.set_code" || (id == "node.add" && props["code"].is_string()) {
                    let code = props["code"].as_str().unwrap_or_default();
                    answer["shader"] = match velo_engine::own(code).and_then(|n| n.validate().map(|_| ())) {
                        Ok(()) => json!("compiles"),
                        Err(e) => json!({ "error": e.to_string(), "line": e.line }),
                    };
                }
                if id == "node.add" {
                    answer["node"] = json!(self.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()));
                }
                if id == "scene.add" {
                    answer["scene"] = json!(self.session.active_scene);
                }
                Ok(answer)
            }
            "checkpoint" => {
                self.session.close_merge();
                Ok(json!({ "ok": true }))
            }
            "undo" => {
                if !prefs.edit {
                    return Err(read_only());
                }
                self.session.close_merge();
                self.session.run("edit.undo", json!({}))?;
                Ok(json!({ "ok": true, "next_undo": self.session.undo_label() }))
            }
            "query_project" => Ok(self.query_project(p)),
            "list_nodes" => Ok(list_nodes()),
            "shader_errors" => Ok(json!(self.live.as_ref().map(|l| l.errors.iter().map(|(node, e)| json!({ "node": node, "error": e.to_string(), "line": e.line })).collect::<Vec<_>>()).unwrap_or_default())),
            "snapshot" => self.snapshot(),
            "listen" => {
                let live = self.live.as_ref().ok_or("the engine isn't running")?;
                let sources = live.sources();
                Ok(json!({
                    "sources": SOURCES.iter().zip(sources).map(|(s, v)| (s.to_string(), json!((v * 100.0).round() / 100.0))).collect::<serde_json::Map<_, _>>(),
                    "input": live.audio.state.lock().map(|s| s.name.clone()).unwrap_or_default(),
                }))
            }
            "save_project" => {
                if !prefs.files {
                    return Err(tr!("This agent may not save: the user can allow it in Preferences → AI and agents").into());
                }
                if self.file.is_none() {
                    return Err(tr!("The project has no file yet: the user saves it once (Cmd+S) first").into());
                }
                if self.save() { Ok(json!({ "ok": true })) } else { Err("saving failed".into()) }
            }
            _ => Err(format!("unknown method '{method}'")),
        }
    }

    /// The project as an agent wants it: a summary with the selected scene in detail, one
    /// scene with everything (code included), or all of it.
    pub fn query_project(&self, p: &Value) -> Value {
        let s = &self.session;
        let live = self.live.as_ref().and_then(|l| l.live_scene().map(String::from));
        let scene_detail = |scene: &velo_core::Scene, code: bool| {
            json!({
                "id": scene.id,
                "name": scene.name,
                "live": live.as_deref() == Some(scene.id.as_str()),
                "nodes": scene.nodes.iter().map(|n| {
                    let mut v = json!({ "id": n.id, "kind": n.kind, "name": n.name, "params": n.params, "mappings": n.mappings, "pos": n.pos });
                    if is_shader(&n.kind) {
                        v["code"] = if code { json!(n.code) } else { json!(n.code.as_ref().map(|c| format!("{} lines; query_project what=scene for the code", c.lines().count()))) };
                        v["error"] = json!(self.live.as_ref().and_then(|l| l.errors.get(&n.id)).map(ToString::to_string));
                    }
                    v
                }).collect::<Vec<_>>(),
                "wires": scene.edges.iter().map(|e| json!({ "from": e.from, "to": e.to })).collect::<Vec<_>>(),
                "macros": scene.macros.iter().enumerate().filter(|(_, m)| !m.targets.is_empty() || !m.name.is_empty()).map(|(i, m)| json!({ "index": i, "name": m.name, "value": m.value, "targets": m.targets })).collect::<Vec<_>>(),
                "variations": scene.variations.iter().map(|v| v.name.clone()).collect::<Vec<_>>(),
                "launch": scene.launch,
            })
        };
        match p["what"].as_str().unwrap_or("summary") {
            "full" => serde_json::to_value(&s.project).unwrap_or_default(),
            "scene" => match p["scene"].as_str().map_or(s.active(), |id| s.project.scene(id)) {
                Some(scene) => scene_detail(scene, true),
                None => json!({ "error": "no such scene" }),
            },
            _ => json!({
                "name": s.project.name,
                "canvas": s.project.canvas,
                "scenes": s.project.scenes.iter().map(|c| json!({ "id": c.id, "name": c.name, "nodes": c.nodes.len(), "live": live.as_deref() == Some(c.id.as_str()) })).collect::<Vec<_>>(),
                "selected": s.active().map(|c| scene_detail(c, false)),
                "bindings": s.project.bindings.len(),
                "undo": s.undo_label(),
            }),
        }
    }

    /// What the preview shows now, as a PNG.
    fn snapshot(&self) -> Result<Value, String> {
        let live = self.live.as_ref().ok_or("the engine isn't running")?;
        let [w, h] = PREVIEW;
        if live.frame.len() != (w * h * 4) as usize {
            return Err("no picture yet: try again in a moment".into());
        }
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().and_then(|mut w| w.write_image_data(&live.frame)).map_err(|e| e.to_string())?;
        }
        let shows = self.session.active().map(|s| s.name.clone());
        Ok(json!({ "png_base64": base64(&png), "width": w, "height": h, "shows": shows }))
    }

    /// Preferences → AI and agents.
    pub fn prefs_agents(&mut self, ui: &mut Ui, t: &Theme) {
        kit::section(ui, t, icons::ROBOT, tr!("AGENTS (MCP)"), true, |ui| {
            let a = &mut self.settings.agent;
            let detail = tr!("Claude Code, Codex or any MCP client can run commands, read the project, write shaders and see the preview. Each change undoes in one step.");
            kit::setting(ui, t, tr!("Let agents use Velo"), detail, |ui| kit::switch(ui, t, &mut a.enabled, tr!("Let agents use Velo")));
            ui.add_enabled_ui(a.enabled, |ui| {
                kit::setting(ui, t, tr!("May change the project"), "", |ui| kit::switch(ui, t, &mut a.edit, tr!("May change the project")));
                kit::setting(ui, t, tr!("May change the live scene"), tr!("Off, they leave the scene the audience sees alone."), |ui| kit::switch(ui, t, &mut a.live, tr!("May change the live scene")));
                kit::setting(ui, t, tr!("May save the project"), "", |ui| kit::switch(ui, t, &mut a.files, tr!("May save the project")));
                kit::setting(ui, t, tr!("Port"), "", |ui| ui.add(egui::DragValue::new(&mut a.port).range(1024..=65535)));
            });
            let status = match self.agents_connected() {
                Some(0) => tr!("Listening on 127.0.0.1:{} · no agent yet", self.settings.agent.port),
                Some(n) => tr!("Listening on 127.0.0.1:{} · {} connected", self.settings.agent.port, n),
                None => tr!("Off").to_string(),
            };
            kit::note(ui, t, Some(icons::LIGHTNING), &status, if self.agents_connected().is_some() { t.accent } else { t.fg_muted });
        });
        kit::section(ui, t, icons::TERMINAL_WINDOW, tr!("SET UP"), false, |ui| {
            kit::hint(ui, t, tr!("With the switch above on, other apps drive Velo through velo-mcp, the bridge that ships with it. Velo's own AI panel needs none of this."));
            kit::hint(ui, t, tr!("Claude Code: run this once in a terminal, then start a new session. --scope user adds Velo in every folder; without it, only in the folder you ran it in."));
            let mut copied = None;
            for (text, json) in [(claude_command(), false), (mcp_json(), true)] {
                if json {
                    kit::hint(ui, t, tr!("Claude Desktop (Settings → Developer → Edit Config), Cursor and other MCP apps: add this to their MCP servers, then restart them."));
                }
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if kit::button(ui, t, Some(icons::COPY), tr!("Copy"), false).clicked() {
                            copied = Some(text.clone());
                        }
                        // the file's entry on one line here; the copy keeps its lines
                        let shown = if json { text.split_whitespace().collect::<Vec<_>>().join(" ") } else { text.clone() };
                        ui.add(egui::Label::new(RichText::new(shown).font(Theme::mono(t.text_xs))).truncate());
                    });
                });
            }
            if let Some(text) = copied {
                ui.ctx().copy_text(text);
                self.say(Tone::Done, tr!("Copied"));
            }
        });
        kit::section(ui, t, icons::LIST_BULLETS, tr!("LOG"), false, |ui| {
            // a box the section's width, framed as the code editor is
            egui::Frame::new().fill(t.bg_field).stroke(egui::Stroke::new(1.0, t.line)).corner_radius(t.radius_sm).inner_margin(egui::Margin::symmetric(t.pad as i8, t.gap as i8)).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).min_scrolled_height(180.0).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    if self.agent.log.is_empty() {
                        ui.label(RichText::new(tr!("Every call an agent makes shows up here.")).font(Theme::mono(t.text_xs)).color(t.fg_muted));
                    }
                    for line in &self.agent.log {
                        let color = if line.ok { t.fg_text } else { t.error };
                        let at = format!("{:02}:{:02}", (line.at / 60.0) as u32, line.at as u32 % 60);
                        ui.add(egui::Label::new(RichText::new(format!("{at}  {}", line.what)).font(Theme::mono(t.text_xs)).color(color)).truncate());
                    }
                });
            });
            if !self.agent.log.is_empty() && kit::button(ui, t, Some(icons::TRASH), tr!("Clear the log"), false).clicked() {
                self.agent.log.clear();
            }
        });
    }
}

/// Every node kind with its inputs and parameters (id, type, range, default), the audio
/// sources a mapping can use, and how a Shader node's code is written.
fn list_nodes() -> Value {
    let kinds: Vec<Value> = NODE_KINDS
        .iter()
        .map(|k| {
            let description = velo_engine::builtin(k.id).and_then(|b| BUILTINS.iter().find(|s| s.0 == b)).and_then(|s| ShaderNode::parse(s.1).ok()).map(|n| n.description);
            let params: Vec<Value> =
                live::shader(k.id, None).map(|s| s.controls.iter().map(|(p, _)| json!({ "id": p.id, "type": format!("{:?}", p.ty).to_lowercase(), "min": p.min, "max": p.max, "default": p.default })).collect()).unwrap_or_default();
            let renders = velo_engine::builtin(k.id).is_some() || is_shader(k.id);
            json!({ "kind": k.id, "label": k.label, "category": k.category.label(), "inputs": k.inputs, "params": params, "description": description, "renders": renders })
        })
        .collect();
    json!({
        "kinds": kinds,
        "sources": SOURCES,
        "wiring": "edge.connect { from: [node, 0], to: [node, input port index] }. A scene shows what feeds its Output node.",
        "shader": shader_format(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn the_live_scene_is_kept_from_scene_edits_only() {
        for id in ["node.add", "node.set_code", "edge.connect", "mapping.add", "macro.set", "variation.store", "scene.rename", "scene.remove"] {
            assert!(edits_a_scene(id), "{id}");
        }
        for id in ["scene.add", "project.set_canvas", "binding.add", "edit.undo"] {
            assert!(!edits_a_scene(id), "{id}");
        }
        // every operator an agent may run is one of the two
        assert!(OPERATORS.iter().any(|o| edits_a_scene(o.id)));
    }

    #[test]
    fn agents_learn_every_node_and_the_shader_format() {
        let nodes = list_nodes();
        let kinds = nodes["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), NODE_KINDS.len());
        let kaleido = kinds.iter().find(|k| k["kind"] == "velo.kaleido").unwrap();
        assert!(kaleido["params"].as_array().unwrap().iter().any(|p| p["id"] == "segments" && p["type"] == "int"));
        assert!(kaleido["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert!(nodes["shader"]["available"].as_str().unwrap().contains("kick"));
    }
}
