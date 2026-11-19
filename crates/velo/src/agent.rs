//! Agents (M5, from Ravune's agent.rs): Velo answers MCP clients (Claude Code, Codex, any MCP
//! client) through `velo-mcp`, a small stdio bridge that talks to this JSON-lines server on
//! localhost. Off until turned on in Preferences → AI and agents. Agents always read; the user decides
//! whether they may edit, touch the live scene, or save. What an agent changes until its next
//! checkpoint undoes in one step, and every call lands in a log the user can read.

use std::io::{BufRead, BufReader, Read, Write};
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
use velo_engine::effects::EFFECT_PRESETS;
use velo_engine::presets::PRESETS;
use velo_engine::shader::ParamType;
use velo_engine::{BUILTINS, GLOBALS, PREVIEW, ShaderNode};

use crate::shell::Tone;
use crate::templates::{RECIPES, TEMPLATES};
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

/// The longest hello line read: a token and a few flags.
const HELLO_MOST: u64 = 4096;

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
    // before the token anything on this computer may be talking: a short line, soon, or nothing
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    if (&mut reader).take(HELLO_MOST).read_line(&mut first).is_err() || !first.ends_with('\n') {
        return;
    }
    // an agent may sit quiet as long as it likes
    let _ = reader.get_ref().set_read_timeout(None);
    let lines = reader.lines();
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

/// The preview as a PNG, for an agent: what it shows now.
fn snapshot_png((frame, shows): (Vec<u8>, Option<String>)) -> Result<Value, String> {
    let [w, h] = PREVIEW;
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().and_then(|mut w| w.write_image_data(&frame)).map_err(|e| e.to_string())?;
    }
    Ok(json!({ "png_base64": base64(&png), "width": w, "height": h, "shows": shows }))
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
        "file": "WGSL. First a /*{ JSON }*/ header: name, category (generator | effect), description, params [{ id, type: float|int|bool|color|vec2, default, min, max }], inputs [{ id: \"src\", type: \"texture\" }] (at most two: src, then a map). Then fn main_image(uv: vec2f, px: vec2f) -> vec4f.",
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
            // a snapshot's PNG is made on a thread of its own, which answers: here it held a frame up
            let answers = call.method == "snapshot";
            let result = if answers {
                self.snapshot_frame().map(|frame| {
                    let reply = call.reply.clone();
                    std::thread::spawn(move || reply.send(snapshot_png(frame)));
                    Value::Null
                })
            } else {
                self.agent_call(&call.method, &call.params, call.panel)
            };
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
            if !answers || result.is_err() {
                let _ = call.reply.send(result);
            }
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
                let mut props = p.get("props").filter(|v| !v.is_null()).cloned().unwrap_or(json!({}));
                // a preset by name: its code, and its name for the node
                if id == "node.add"
                    && let Some(slug) = props.as_object_mut().and_then(|o| o.remove("preset"))
                {
                    let slug = slug.as_str().unwrap_or_default();
                    let kind = props["kind"].as_str().unwrap_or(velo_core::SHADER).to_string();
                    let (code, name) = preset(&kind, slug).ok_or_else(|| format!("no {kind} preset {slug}: list_nodes names them"))?;
                    props["kind"] = json!(if is_shader(&kind) { kind } else { velo_core::SHADER.to_string() });
                    props["code"] = json!(code);
                    if props["name"].is_null() {
                        props["name"] = json!(name);
                    }
                }
                // the live scene is the audience's: outside agents work on the others unless
                // allowed; Velo's own AI panel may (the user is asking, in Velo)
                let target = props["scene"].as_str().map(String::from).or_else(|| self.session.active_scene.clone());
                let live = self.live.as_ref().and_then(|l| l.live_scene().map(String::from));
                // the shared signals and the canvas's size reach the live scene too
                let shown = live.is_some() && ((edits_a_scene(id) && (target == live || target.as_deref() == Some(velo_core::SHARED))) || id == "project.set_canvas");
                if !prefs.live && !panel && shown {
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
            "generate_image" => {
                if !prefs.edit {
                    return Err(read_only());
                }
                let node = p["node"].as_str().ok_or("generate_image needs an AI Image node's id (node.add with kind velo.ai_image makes one)")?;
                let (scene, _) = self.node_scene(node).ok_or_else(|| format!("no node {node}"))?;
                // what it is before its prompt is touched: the error says what's wrong
                if self.session.project.graphs().flat_map(|s| &s.nodes).any(|n| n.id == node && n.kind != velo_core::AI_IMAGE) {
                    return Err(format!("{node} isn't an AI Image node"));
                }
                // a new prompt is an edit like any: the live scene is the audience's
                if let Some(prompt) = p["prompt"].as_str() {
                    let live = self.live.as_ref().and_then(|l| l.live_scene().map(String::from));
                    if !prefs.live && !panel && live.as_deref() == Some(scene.as_str()) {
                        return Err(tr!("That scene is live: work on another one (scene.add makes a new one and selects it; the preview shows it), or the user allows editing live in Preferences → AI and agents").into());
                    }
                    self.session.run_merge("node.set_text", json!({ "node": node, "text": prompt, "scene": scene }), MERGE)?;
                }
                let count = p["count"].as_u64().map_or(1, |c| c as usize);
                let asked = self.generate(node, count)?;
                Ok(json!({
                    "ok": true,
                    "making": asked,
                    "note": "the pictures are made on this computer, one after another (seconds each), and go into the node as they come, played in turn: don't wait for them; generation_status says how far it is",
                }))
            }
            "generation_status" => Ok(self.generation_status(p["node"].as_str())),
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
            "describe_node" => describe_node(p),
            "use_recipe" => {
                if !prefs.edit {
                    return Err(read_only());
                }
                let name = p["name"].as_str().ok_or("use_recipe needs a recipe's name from list_nodes")?;
                let recipe = TEMPLATES.iter().chain(&RECIPES).find(|r| r.name.eq_ignore_ascii_case(name)).ok_or_else(|| format!("no recipe {name}: list_nodes has them"))?;
                crate::templates::add(&mut self.session, recipe, Some(MERGE))?;
                let scene = self.session.active().ok_or("no scene")?;
                Ok(json!({
                    "ok": true,
                    "scene": scene.id,
                    "nodes": scene.nodes.iter().map(|n| json!({ "id": n.id, "kind": n.kind })).collect::<Vec<_>>(),
                    "next": "a new scene, selected: tune it (node.set_param, mapping.add, more nodes) and look with snapshot",
                }))
            }
            "shader_errors" => Ok(json!(self.live.as_ref().map(|l| l.errors.iter().map(|(node, e)| json!({ "node": node, "error": e.to_string(), "line": e.line })).collect::<Vec<_>>()).unwrap_or_default())),
            "snapshot" => self.snapshot_frame().and_then(snapshot_png),
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
                // the signal nodes every scene reads, as "shared:<node id>/<output>"
                "shared": json!({
                    "nodes": s.project.shared.nodes.iter().map(|n| json!({ "id": n.id, "kind": n.kind, "name": n.name, "params": n.params, "off": n.off })).collect::<Vec<_>>(),
                    "wires": s.project.shared.edges.iter().map(|e| json!({ "from": e.from, "to": e.to })).collect::<Vec<_>>(),
                }),
                "bindings": s.project.bindings.len(),
                "undo": s.undo_label(),
            }),
        }
    }

    /// What the preview shows now (its pixels, and the scene's name), for [`snapshot_png`].
    fn snapshot_frame(&self) -> Result<(Vec<u8>, Option<String>), String> {
        let live = self.live.as_ref().ok_or("the engine isn't running")?;
        let [w, h] = PREVIEW;
        if live.frame.len() != (w * h * 4) as usize {
            return Err("no picture yet: try again in a moment".into());
        }
        Ok((live.frame.clone(), self.session.active().map(|s| s.name.clone())))
    }

    /// Preferences → AI and agents.
    pub fn prefs_agents(&mut self, ui: &mut Ui, t: &Theme) {
        kit::page_title(ui, t, icons::ROBOT, tr!("AI and agents"), tr!("Let Claude Code, Codex or any MCP client drive Velo."));
        kit::card(ui, t, tr!("AGENTS (MCP)"), |ui| {
            let a = &mut self.settings.agent;
            let detail = tr!("Claude Code, Codex or any MCP client can run commands, read the project, write shaders and see the preview. Each change undoes in one step.");
            kit::setting(ui, t, tr!("Let agents use Velo"), detail, |ui| kit::switch(ui, t, &mut a.enabled, tr!("Let agents use Velo")));
            ui.add_enabled_ui(a.enabled, |ui| {
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("May change the project"), "", |ui| kit::switch(ui, t, &mut a.edit, tr!("May change the project")));
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("May change the live scene"), tr!("Off, they leave the scene the audience sees alone."), |ui| kit::switch(ui, t, &mut a.live, tr!("May change the live scene")));
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("May save the project"), "", |ui| kit::switch(ui, t, &mut a.files, tr!("May save the project")));
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("Port"), "", |ui| ui.add(egui::DragValue::new(&mut a.port).range(1024..=65535)));
            });
            let status = match self.agents_connected() {
                Some(0) => tr!("Listening on 127.0.0.1:{} · no agent yet", self.settings.agent.port),
                Some(n) => tr!("Listening on 127.0.0.1:{} · {} connected", self.settings.agent.port, n),
                None => tr!("Off").to_string(),
            };
            kit::callout(ui, t, icons::LIGHTNING, &status, if self.agents_connected().is_some() { t.accent } else { t.fg_muted });
            ui.add_space(t.pad);
        });
        self.prefs_images(ui, t);
        kit::card(ui, t, tr!("SET UP"), |ui| {
            kit::para(ui, t, tr!("With the switch above on, other apps drive Velo through velo-mcp, the bridge that ships with it. Velo's own AI panel needs none of this."), t.fg_text);
            ui.add_space(t.gap);
            let mut copied = None;
            for (text, json) in [(claude_command(), false), (mcp_json(), true)] {
                let how = if json {
                    tr!("Claude Desktop (Settings → Developer → Edit Config), Cursor and other MCP apps: add this to their MCP servers, then restart them.")
                } else {
                    tr!("Claude Code: run this once in a terminal, then start a new session. --scope user adds Velo in every folder; without it, only in the folder you ran it in.")
                };
                kit::hint(ui, t, how);
                ui.add_space(t.gap);
                // the file's entry on one line here; the copy keeps its lines
                let shown = if json { text.split_whitespace().collect::<Vec<_>>().join(" ") } else { text.clone() };
                if kit::snippet(ui, t, &shown) {
                    copied = Some(text.clone());
                }
                ui.add_space(t.pad);
            }
            if let Some(text) = copied {
                ui.ctx().copy_text(text);
                self.say(Tone::Done, tr!("Copied"));
            }
        });
        kit::card(ui, t, tr!("LOG"), |ui| {
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
            if !self.agent.log.is_empty() {
                ui.add_space(t.gap);
                if kit::button(ui, t, Some(icons::TRASH), tr!("Clear the log"), false).clicked() {
                    self.agent.log.clear();
                }
            }
            ui.add_space(t.pad);
        });
    }
}

/// What an agent reads first: every node kind in a line, the recipes built from them, the
/// shader presets by name, the audio sources. A node's parameters come from [`describe_node`],
/// for the kinds it picks: the whole catalog at once is some 15k tokens.
pub(crate) fn list_nodes() -> Value {
    let kinds: Vec<Value> = NODE_KINDS
        .iter()
        .map(|k| {
            let mut v = json!({ "kind": k.id, "label": k.label, "group": k.group(), "about": crate::canvas::node_about(k.id).unwrap_or_default() });
            if !k.inputs.is_empty() {
                v["inputs"] = json!(k.inputs);
            }
            if k.outputs != ["out"] {
                v["outputs"] = json!(k.outputs);
            }
            v
        })
        .collect();
    let recipes: Vec<Value> = TEMPLATES.iter().chain(&RECIPES).map(|r| json!({ "name": r.name, "about": r.description, "patch": r.outline() })).collect();
    let names = |presets: &[(&str, &str)]| presets.iter().map(|p| p.0).collect::<Vec<_>>().join(", ");
    json!({
        "kinds": kinds,
        "recipes": recipes,
        "shader_presets": names(&PRESETS),
        "effect_presets": names(&EFFECT_PRESETS),
        "operators": common_operators(),
        "sources": SOURCES,
        "wiring": "Pictures: edge.connect { from: [node, 0], to: [node, input port index] }; a scene shows what feeds its Output node. Signals (numbers): a signal node's output moves a parameter through mapping.add with source \"<node id>/<output index>\" (or a source name: kick, bass…); signal nodes feed each other with edge.connect.",
        "how": "operators has the usual edits with example props: run them with run_operator { id, props }; search_operators finds the rest. Build from these nodes: a look is usually a generator, a few effects and signal nodes moving them. use_recipe starts from a recipe as a new scene. A preset is a ready Shader node: node.add with kind velo.shader (effect presets: velo.shader_fx) and preset, its name. describe_node gives the parameters, ranges and named choices of the kinds you'll use. Write a Shader node's own WGSL only when no node, recipe or preset does it (describe_node velo.shader has the format).",
    })
}

/// The operators nearly every build runs, with their example props: an agent that has them
/// needn't search for them (search_operators has the rest).
const COMMON_OPERATORS: [&str; 9] = ["scene.add", "scene.rename", "node.add", "node.set_param", "node.remove", "edge.connect", "mapping.add", "mapping.set", "mapping.remove"];

fn common_operators() -> Value {
    COMMON_OPERATORS.iter().filter_map(|id| OPERATORS.iter().find(|o| o.id == *id)).map(|o| (o.id.to_string(), serde_json::from_str(o.example).unwrap_or(json!(o.example)))).collect::<serde_json::Map<_, _>>().into()
}

/// The kinds an agent picked, whole: their parameters (id, type, range, default and the names
/// of an int's choices), a built-in shader's longer description, and for a Shader node how its
/// code is written and the presets it can run.
fn describe_node(p: &Value) -> Result<Value, String> {
    let asked: Vec<&str> = match (&p["kinds"], &p["kind"]) {
        (Value::Array(a), _) => a.iter().filter_map(Value::as_str).collect(),
        (Value::String(s), _) | (_, Value::String(s)) => vec![s.as_str()],
        _ => Vec::new(),
    };
    if asked.is_empty() {
        return Err("describe_node needs kinds: node kinds from list_nodes, e.g. [\"velo.polar\", \"velo.lfo\"]".into());
    }
    let round = |x: f32| ((x as f64) * 1000.0).round() / 1000.0;
    let kinds = asked
        .iter()
        .map(|id| {
            let k = NODE_KINDS.iter().find(|k| k.id == *id).ok_or_else(|| format!("no node kind {id}: list_nodes has them"))?;
            let params: Vec<Value> = live::shader(k.id, None)
                .map(|s| {
                    s.controls
                        .iter()
                        .map(|(p, _)| {
                            let default = match p.ty {
                                ParamType::Color | ParamType::Vec4 => json!(p.default.map(round)),
                                ParamType::Vec2 => json!([round(p.default[0]), round(p.default[1])]),
                                _ => json!(round(p.default[0])),
                            };
                            let mut v = json!({ "id": p.id, "type": format!("{:?}", p.ty).to_lowercase(), "min": round(p.min), "max": round(p.max), "default": default });
                            if !p.names.is_empty() {
                                v["choices"] = json!(p.names);
                            }
                            v
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut v = json!({ "kind": k.id, "label": k.label, "about": crate::canvas::node_about(k.id).unwrap_or_default(), "inputs": k.inputs, "outputs": k.outputs, "params": params });
            if let Some(d) = velo_engine::builtin(k.id).and_then(|b| BUILTINS.iter().find(|s| s.0 == b)).and_then(|s| ShaderNode::parse(s.1).ok()).map(|n| n.description) {
                v["description"] = json!(d);
            }
            if is_shader(k.id) {
                let presets = if k.id == velo_core::SHADER_FX { &EFFECT_PRESETS[..] } else { &PRESETS[..] };
                v["presets"] = presets.iter().filter_map(|(slug, code)| ShaderNode::parse(code).ok().map(|n| json!({ "preset": slug, "about": n.description }))).collect();
                v["shader"] = shader_format();
            }
            Ok(v)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(json!({ "kinds": kinds }))
}

/// A preset's code and name, by its slug: a Shader node's (`velo.shader`) or a Shader FX's.
fn preset(kind: &str, slug: &str) -> Option<(&'static str, String)> {
    let presets = if kind == velo_core::SHADER_FX { &EFFECT_PRESETS[..] } else { &PRESETS[..] };
    let code = presets.iter().find(|p| p.0.eq_ignore_ascii_case(slug) || p.0.replace('_', "-") == slug.replace('_', "-"))?.1;
    Some((code, ShaderNode::parse(code).map_or_else(|_| slug.to_string(), |n| n.name)))
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
        // the index: every kind in a line, each with what it does, and small enough to read first
        let nodes = list_nodes();
        let kinds = nodes["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), NODE_KINDS.len());
        assert!(kinds.iter().all(|k| k["about"].as_str().is_some_and(|a| !a.is_empty())), "every kind says what it does");
        assert!(nodes.to_string().len() < 17_000, "the index stays small: {} bytes", nodes.to_string().len());
        for id in COMMON_OPERATORS {
            assert!(nodes["operators"][id].is_object(), "{id} and its example props");
        }
        assert!(nodes["shader_presets"].as_str().unwrap().contains("tunnel") && nodes["effect_presets"].as_str().unwrap().contains("glitch"));
        let recipes = nodes["recipes"].as_array().unwrap();
        assert_eq!(recipes.len(), TEMPLATES.len() + RECIPES.len());
        let tunnel = recipes.iter().find(|r| r["name"] == "Ring Tunnel").unwrap();
        assert_eq!(tunnel["patch"], "Gradient → Polar; Polar → Feedback; Feedback → Output; kick moves Polar zoom; LFO moves Polar turn");
        // the kinds asked for, whole
        let asked = describe_node(&json!({ "kinds": ["velo.kaleido", "velo.lfo", "velo.shader"] })).unwrap();
        let [kaleido, lfo, shader] = &asked["kinds"].as_array().unwrap()[..] else { panic!() };
        assert!(kaleido["params"].as_array().unwrap().iter().any(|p| p["id"] == "segments" && p["type"] == "int"));
        assert!(kaleido["description"].as_str().is_some_and(|d| !d.is_empty()));
        assert!(lfo["params"].as_array().unwrap().iter().any(|p| p["id"] == "shape" && p["choices"][0] == "Sine"));
        assert!(shader["shader"]["available"].as_str().unwrap().contains("kick"));
        assert!(shader["presets"].as_array().unwrap().iter().any(|p| p["preset"] == "tunnel"));
        assert!(describe_node(&json!({ "kind": "velo.nope" })).is_err());
        assert!(describe_node(&json!({})).is_err());
        // a preset by name, for either shader kind
        assert!(preset(velo_core::SHADER, "starfield").is_some());
        assert!(preset(velo_core::SHADER_FX, "chromatic_aberration").is_some(), "its slug's - or _");
        assert!(preset(velo_core::SHADER, "glitch").is_none(), "an effect isn't a Shader node's");
    }
}
