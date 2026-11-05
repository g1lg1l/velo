//! `velo-mcp`: an MCP server on stdio that drives a running Velo (from Ravune's `ravune-mcp`).
//! MCP clients start it (Claude Code: `claude mcp add velo -- <path>/velo-mcp`); it reads the
//! port and the secret Velo wrote into its data folder (`agent.json`, there while Preferences →
//! Agents is on) and forwards each tool call to Velo as one JSON line.

use std::io::{BufRead, Write};

use serde_json::{Value, json};
use velo_mcp::{INSTRUCTIONS, PROTOCOLS, Velo, tools};

#[derive(Default)]
struct Bridge {
    velo: Option<Velo>,
    /// Velo's data folder when it isn't the usual one (`--data`, which Velo passes: clients
    /// start bridges with an environment of their own, `VELO_DATA` may not reach it).
    data: Option<std::path::PathBuf>,
    /// Started by Velo's AI panel (`--panel`).
    panel: bool,
}

impl Bridge {
    /// Forwards a tool call, reconnecting once if Velo restarted.
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        for attempt in 0..2 {
            if self.velo.is_none() {
                self.velo = Some(Velo::connect(self.data.as_deref(), self.panel)?);
            }
            match self.velo.as_mut().map(|v| v.call(name, args.clone())) {
                Some(Err(e)) if attempt == 0 && (e.contains("closed") || e.contains("os error")) => self.velo = None,
                Some(result) => return result,
                None => unreachable!("connected above"),
            }
        }
        Err("Velo isn't answering".into())
    }

    /// One JSON-RPC message in, the reply out (none for notifications).
    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?.clone();
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => {
                let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
                let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "velo", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.tool(name, args) {
                    // a snapshot is a picture the model can look at
                    Ok(v) if v["png_base64"].is_string() => json!({
                        "content": [
                            { "type": "image", "data": v["png_base64"], "mimeType": "image/png" },
                            { "type": "text", "text": format!("{}×{} preview of {}", v["width"], v["height"], v["shows"].as_str().unwrap_or("the selected scene")) },
                        ],
                        "isError": false,
                    }),
                    // a video's frames: each a picture, then what they are
                    Ok(v) if v["frames"].is_array() => {
                        let frames = v["frames"].as_array().cloned().unwrap_or_default();
                        let times: Vec<String> = frames.iter().map(|f| format!("{}s", f["time"])).collect();
                        let mut content: Vec<Value> = frames.iter().map(|f| json!({ "type": "image", "data": f["jpeg_base64"], "mimeType": "image/jpeg" })).collect();
                        content.push(json!({ "type": "text", "text": format!("{}: {}×{}, {}s long; frames at {}", v["path"].as_str().unwrap_or("the video"), v["width"], v["height"], v["length"], times.join(", ")) }));
                        json!({ "content": content, "isError": false })
                    }
                    Ok(v) => json!({ "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }], "isError": false }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                })
            }
            other => Err(json!({ "code": -32601, "message": format!("no method '{other}'") })),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
        })
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = args.iter().position(|a| a == "--data").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let mut bridge = Bridge { data, panel: args.iter().any(|a| a == "--panel"), ..Default::default() };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => bridge.handle(&msg),
            Err(e) => Some(json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": e.to_string() } })),
        };
        if let Some(reply) = reply
            && writeln!(stdout, "{reply}").and_then(|_| stdout.flush()).is_err()
        {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;
    use std::net::TcpListener;

    /// A fake Velo: checks the token, then echoes each method and its params back; a snapshot
    /// answers a picture.
    #[test]
    fn speaks_mcp_and_forwards_tool_calls() {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("velo-mcp-test-{port}"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("agent.json"), json!({ "port": port, "token": "s3cret" }).to_string()).unwrap();
        // SAFETY: this test is the only one touching the environment
        unsafe { std::env::set_var("VELO_DATA", &dir) };
        std::thread::spawn(move || {
            let (stream, _) = server.accept().unwrap();
            let mut out = stream.try_clone().unwrap();
            for line in BufReader::new(stream).lines() {
                let msg: Value = serde_json::from_str(&line.unwrap()).unwrap();
                let reply = match msg["method"].as_str().unwrap() {
                    "hello" => {
                        assert_eq!(msg["params"]["token"], "s3cret");
                        json!({ "id": msg["id"], "result": {} })
                    }
                    "undo" => json!({ "id": msg["id"], "error": "nothing to undo" }),
                    "snapshot" => json!({ "id": msg["id"], "result": { "png_base64": "iVBORw0K", "width": 640, "height": 360, "shows": "Tunnel" } }),
                    "video_frames" => json!({ "id": msg["id"], "result": { "path": "ref.mp4", "width": 96, "height": 48, "length": 0.4, "frames": [{ "time": 0.0, "jpeg_base64": "/9j/" }, { "time": 0.2, "jpeg_base64": "/9j/" }] } }),
                    _ => json!({ "id": msg["id"], "result": { "method": msg["method"], "params": msg["params"] } }),
                };
                writeln!(out, "{reply}").unwrap();
            }
        });
        let mut b = Bridge::default();
        let init = b.handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } })).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(b.handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none(), "no reply to a notification");
        let list = b.handle(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 11);
        let call = b.handle(&json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "run_operator", "arguments": { "id": "scene.add" } } })).unwrap();
        assert_eq!(call["result"]["isError"], false);
        let text: Value = serde_json::from_str(call["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!((text["method"].as_str(), text["params"]["id"].as_str()), (Some("run_operator"), Some("scene.add")));
        let picture = b.handle(&json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "snapshot" } })).unwrap();
        assert_eq!(picture["result"]["content"][0]["type"], "image");
        assert_eq!(picture["result"]["content"][0]["mimeType"], "image/png");
        let video = b.handle(&json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "video_frames", "arguments": { "path": "ref.mp4" } } })).unwrap();
        let content = video["result"]["content"].as_array().unwrap();
        assert_eq!(content.iter().filter(|c| c["mimeType"] == "image/jpeg").count(), 2, "one picture a frame");
        assert!(content[2]["text"].as_str().unwrap().contains("0.2s"), "{content:?}");
        let failed = b.handle(&json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "name": "undo" } })).unwrap();
        assert_eq!((failed["result"]["isError"].as_bool(), failed["result"]["content"][0]["text"].as_str()), (Some(true), Some("nothing to undo")));
        let unknown = b.handle(&json!({ "jsonrpc": "2.0", "id": 6, "method": "resources/list" })).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }
}
