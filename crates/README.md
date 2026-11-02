# Crates

Four libraries and the app. A crate splits off only when the code asks for it. See [ARCHITECTURE.md](../ARCHITECTURE.md) for how they fit together.

| Crate | What it does | Depends on | Must not depend on |
|---|---|---|---|
| `velo-core` | The project model, node kinds, mappings, macros, bindings, scenes, operators and undo, RON files and migrations | serde, ron | wgpu, cpal, egui |
| `velo-audio` | Audio input (devices, the computer's own output, one app on macOS), analysis, the drum model, tempo | cpal, rtrb | wgpu, egui |
| `velo-engine` | The render thread, compiling scenes into GPU graphs, the shader format and presets, modulation, launches, media, Spout and NDI | velo-core, wgpu, naga | egui |
| `velo` | The app: the event loop, the egui UI, MIDI, OSC, Ableton Link, the AI panel, the agents' server, updates | all of the above, eframe | |
| `velo-mcp` | The MCP bridge on stdio: MCP clients start it, it forwards their tool calls to a running Velo over localhost | serde_json | everything else (it stays a small separate binary) |

`velo-engine` runs headless, with no UI: its tests render shaders and graphs on the GPU.
