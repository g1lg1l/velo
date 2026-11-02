//! The MCP side of Velo's agents, shared by the `velo-mcp` bridge and Velo's own AI panel
//! (which drives local models with the same tools): the tool list, what the AI is told about
//! Velo, and the line to a running Velo's agent server.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub const INSTRUCTIONS: &str = "Velo is a live-visuals app: each scene is a graph of GPU nodes (generators, effects, a Shader node running \
your own WGSL) wired into an Output, moved by the music through mappings (a source like kick or bass moves a parameter) and macros. \
Every edit is an operator: find them with search_operators, run them with run_operator. Start with list_nodes (every node kind, its \
parameters, the audio sources, how to write a shader) and query_project. To make something new: scene.add (it becomes the selected \
scene, which the preview shows), node.add, edge.connect into an Output, node.set_param, mapping.add. Use the existing nodes \
whenever they can do the job; write a Shader node's WGSL only when none does. Look at the result with snapshot. \
The live scene is the audience's: work on another one unless the user allowed otherwise. Everything you change until checkpoint is \
one undo step for the user.";

/// Velo's data folder, as the app finds it (eframe's storage dir, or `VELO_DATA`).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("VELO_DATA") {
        return Some(dir.into());
    }
    let home = || std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Velo").join("data"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library").join("Application Support").join("Velo"))
    } else {
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home().map(|h| h.join(".local").join("share"))).map(|d| d.join("velo"))
    }
}

pub fn tools() -> Value {
    let object = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    let none = || object(json!({}), &[]);
    json!([
        { "name": "list_nodes", "description": "Every node kind Velo has: inputs, parameters (id, type, range, default), what it does; the audio sources mappings can use; how to write a Shader node's WGSL, with an example.",
          "inputSchema": none() },
        { "name": "query_project", "description": "Read the project: a summary (scenes, which is live, the selected one in detail: nodes, params, mappings, wires, macros), one scene with everything including shader code, or all of it.",
          "inputSchema": object(json!({ "what": { "type": "string", "enum": ["summary", "scene", "full"] }, "scene": { "type": "string", "description": "a scene id for what=scene (default: the selected one)" } }), &[]) },
        { "name": "search_operators", "description": "Find Velo operators (every edit is one) by words in their id, label or description. Returns ids, descriptions and example props.",
          "inputSchema": object(json!({ "query": { "type": "string", "description": "e.g. \"node\", \"mapping\", \"macro\", \"shader code\"; empty lists all" } }), &[]) },
        { "name": "run_operator", "description": "Run one operator with JSON props (see search_operators for examples). Answers the new node or scene's id, and for shader code whether it compiles. Edits until the next checkpoint undo together with one Ctrl+Z.",
          "inputSchema": object(json!({ "id": { "type": "string" }, "props": { "type": "object" } }), &["id"]) },
        { "name": "snapshot", "description": "Look at what the preview shows now (the selected scene, or the live one while it's selected): a PNG.", "inputSchema": none() },
        { "name": "shader_errors", "description": "Shader nodes whose code doesn't compile in the latest build, with the line and naga's message.", "inputSchema": none() },
        { "name": "listen", "description": "What the music is doing right now: each audio source (level, bass, mid, high, kick, snare, hat, beat, bar_phase) 0..1, and the input's name.", "inputSchema": none() },
        { "name": "checkpoint", "description": "Close the current undo step: later edits undo separately.", "inputSchema": none() },
        { "name": "undo", "description": "Take back the last undo step (everything since the last checkpoint, if you edited).", "inputSchema": none() },
        { "name": "save_project", "description": "Save the project to its file (only if the user allowed it in Preferences → AI and agents).", "inputSchema": none() },
    ])
}

/// The line to Velo, opened on the first tool call.
pub struct Velo {
    lines: BufReader<TcpStream>,
    out: TcpStream,
    next: u64,
}

impl Velo {
    /// Connects to the Velo whose data folder is `dir` (default: the one Velo uses). `panel`:
    /// Velo's own AI panel started this bridge (the user is typing in Velo).
    pub fn connect(dir: Option<&Path>, panel: bool) -> Result<Self, String> {
        let off = "Velo isn't listening: open Velo, then Preferences → AI and agents → \"Let agents use Velo\"";
        let file = dir.map(Path::to_path_buf).or_else(data_dir).ok_or(off)?.join("agent.json");
        let info: Value = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).ok_or(off)?;
        let port = info["port"].as_u64().ok_or(off)? as u16;
        let out = TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("{off} ({e})"))?;
        let lines = BufReader::new(out.try_clone().map_err(|e| e.to_string())?);
        let mut velo = Self { lines, out, next: 1 };
        velo.call("hello", json!({ "token": info["token"], "panel": panel }))?;
        Ok(velo)
    }

    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        writeln!(self.out, "{}", json!({ "id": id, "method": method, "params": params })).map_err(|e| e.to_string())?;
        let mut line = String::new();
        if self.lines.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("Velo closed the connection".into());
        }
        let reply: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        match reply.get("error") {
            Some(e) => Err(e.as_str().map_or_else(|| e.to_string(), String::from)),
            None => Ok(reply["result"].clone()),
        }
    }
}
