# Velo: Architecture

How Velo is put together, for contributors. Subsystems in detail: [graph](docs/graph.md) · [audio](docs/audio.md) · [rendering](docs/rendering.md) · [control](docs/control.md) · [scenes](docs/scenes.md) · [project format](docs/project-format.md) · [shaders and extensions](docs/plugins.md).

## 1. The shape

```
cpal callback ──ring──► audio thread ──atomics──► render thread ──► output window, Spout, NDI
(downmix only)          (analysis, tempo)          ▲          │
                                                   │          └──► preview pixels ──► UI
MIDI, OSC threads ──► UI thread ──── Command ──────┘
                      (egui, operators,
                       the Project)
                         │      ▲
                   scene ▼      │ built graph
                      compile thread
                      (shaders, pipelines, textures)
```

Three ideas carry the design:

1. **The document is the truth; runtimes are disposable.** The UI edits a plain-data `Project` through operators. The engine only receives built graphs and small value messages, so anything in it can be thrown away and rebuilt from the document (a lost GPU, a restart).
2. **Each thread has one job and never waits for a slower one.** Audio callback, analysis, render and UI are joined by a lock-free ring, atomics and fire-and-forget channels. A slow UI can't stall the output; a stalled analysis leaves the visuals running on the last values.
3. **Hot path and cold path.** Turning a knob is a few bytes on a channel. Changing the structure of a scene is a build on the compile thread, swapped in at a frame boundary.

## 2. Modules

Four library crates and the app; see [crates/README.md](crates/README.md).

| Concern | Code | Thread |
|---|---|---|
| Project model, operators, undo | `velo-core` (`lib.rs`, `ops.rs`) | UI |
| Files: bundles, autosave, crash recovery | `velo/src/files.rs` | UI |
| A scene into a graph the GPU runs | `velo-engine/src/compile.rs` | compile |
| Frame loop, outputs, preview | `velo-engine/src/runtime.rs`, `gpu.rs` | render |
| Beat clock, mappings, macros, signal nodes | `velo-engine/src/modulate.rs`, `signal.rs` | render |
| Scene launches and transitions | `velo-engine/src/launch.rs` | render |
| Shader format, ISF, presets | `velo-engine/src/shader.rs`, `isf.rs`, `presets.rs`, `effects.rs` | compile |
| Pictures, HAP, system video, text | `velo-engine/src/media.rs`, `hap.rs`, `video.rs`, `video_windows.rs`, `text.rs` | media threads |
| Spout and NDI | `velo-engine/src/share.rs` | share |
| Audio input, analysis, drum model, tempo | `velo-audio` | callback, audio |
| The engine and audio as the UI sees them | `velo/src/live.rs` | UI |
| Event loop, output window | `velo/src/host.rs` | main |
| MIDI, OSC, Ableton Link | `velo/src/midi.rs`, `osc.rs`, `link.rs` | their own threads, applied on the UI |
| Canvas, shell, kit, widgets, theme | `velo/src/canvas.rs`, `shell.rs`, `kit.rs`, `widgets.rs`, `theme.rs` | UI |
| AI panel and its providers | `velo/src/chat.rs`, `ai.rs` | UI, ai |
| Agents' server and the MCP bridge | `velo/src/agent.rs`, `velo-mcp` | agents; a separate process |

## 3. Data flow

### 3.1 Operators
Every edit is an operator: an entry in `velo_core::ops::OPERATORS` with an id (`node.add`), a label, a description, example props as JSON, `poll` and `exec`. Menus, keymaps, the command palette, agents over MCP, and MIDI and OSC bindings that change the document all run the same operators, so each gets undo for free (launches, blackout and taps go straight to the engine as commands). Undo keeps whole copies of the `Project` (up to 500 steps); a drag merges into one step (`Session::run_merge`), and an agent's edits merge until its next checkpoint.

### 3.2 From an edit to pixels
```
UI: an operator changes the Project
once a frame, live.rs compares each scene the engine keeps with the document:
 ├─ only values changed (a parameter, a mapping, a macro)
 │     → Command::Params / Mappings / Macro / MacroTargets / Signals
 └─ the structure changed (nodes, wires, kinds, code, off, the canvas size)
       → compile thread: plan, shaders checked by naga, pipelines, textures → Built
       → Command::Install { key, graph }
render thread: swaps the graph in at the next frame boundary
```
Values travel by index (the node, then each parameter in its header's order), never by name. Moving or renaming a node never rebuilds.

### 3.3 From the engine back to the UI
- `Status`: atomics the UI reads without asking: frames, late frames, stalls, frame time, the beat clock, the watched node's modulated values, signal outputs, the live, fading and armed scenes and the launch countdown, each node's GPU cost, a lost device.
- Preview: drawn at 640×360, copied into one of three mappable buffers; the UI copies the newest into an egui texture. The render thread never waits for it. The engine and the UI have separate GPU devices (D16).
- Built graphs come back from the compile thread on a channel, with each Shader node's errors.

## 4. Threads

| Thread | Job | Rule |
|---|---|---|
| cpal callback | Downmix to mono, push into a lock-free ring (`rtrb`) | Never allocates, locks or waits |
| Audio | Analysis, drum model, tempo; features and the waveform out as atomics | Readers never wait for it |
| Render | Commands, pacing, modulation, launches, passes, present | Never compiles or allocates pixels; never waits for another thread; at most two frames on the GPU |
| Main (UI) | winit, egui, operators, bindings, autosave | May block (a dialog); the output never depends on it |
| Compile | Builds graphs with the engine's device | Only the newest build of each scene is made |
| Media, one reader per movie | Decode files; read movie frames as they come due | |
| Share | Sends frames to Spout or NDI | |
| MIDI, OSC | Stamp each message and queue it for the UI | |
| Agents, AI | The MCP server; the AI providers | |

Early prototypes, spikes S1 to S4, settled the output thread (S1), the look (S2), the analysis (S3) and the node canvas (S4). Their code now lives in the app; comments still name them.

### Why threads, not processes
A separate engine process (a UI crash can't kill the output) was considered. Sharing GPU textures between processes needs shared-handle code per platform (IOSurface, DXGI handles, Vulkan external memory) just for the preview, and the UI is not where crashes come from. The engine is a library with a message API, so a headless engine process stays possible later.

## 5. Failure model

| Failure | What happens |
|---|---|
| A shader that doesn't compile | The node keeps its last good program, else passes its input through; the error shows at its line in the code editor |
| A scene that can't be built | That scene reports the error; the other scenes and the output go on |
| A missing media or font file | The node shows black; the project still opens. A missing shader file in a bundle is listed on opening |
| The audio input goes away | Velo asks for it again until it's back |
| A MIDI port goes away | Ports are scanned every second; bindings name a port (or any port) and work again when it returns |
| The GPU device is lost (a driver reset), or the render thread stops drawing for 2 s | The event loop starts a new engine from the document, reopens the output and puts the scene that was out back out; the UI says so |
| The output's display is unplugged | The output closes and the UI says which display |
| A display stops taking frames (covered, stuck) | It is left alone for 250 ms at a time; the graphs and the preview go on; counted as a stall |
| A scene heavier than the GPU | At most two frames in flight: it slows down instead of piling up memory |
| Velo crashes | Unsaved work is autosaved every 60 s; the next start offers it |
| A project from a newer Velo | Refused with a message, never overwritten |

## 6. Rust stack

| Area | Crates | Notes |
|---|---|---|
| GPU | `wgpu` 30, `naga` 30 | Metal, Vulkan, DX12. naga validates WGSL and translates ISF's GLSL |
| Window and UI | `eframe` / `egui` 0.37, `winit` 0.30 | Velo runs its own winit event loop with eframe inside (`host.rs`), so it owns the output window |
| Audio | `cpal` 0.18, `rtrb` 0.4 | Devices and the computer's own output. FFT, filters and the drum model are Velo's own code |
| MIDI | `midir` 0.11 | CoreMIDI, WinMM, ALSA |
| OSC | none | A small parser of its own over UDP |
| Ableton Link | `rusty_link` 0.4 (Ableton's SDK) | The `link` feature, on by default; it makes the app GPL (D11) |
| Files | `serde`, `ron` 0.12, `serde_json` | |
| Media | `image` 0.25, `snap` (HAP), `objc2-av-foundation` (macOS), `windows` (Media Foundation, Spout's DX11) | |
| Text | `ab_glyph` | Glyphs as signed distance fields |
| NDI | `libloading` | The NDI runtime the user installs, loaded when sharing starts |
| Dialogs | `rfd` 0.17 | |
| API keys | `keyring` 4 | The OS keychain, for cloud AI endpoints |
| Updates | `ureq` 3 (rustls), `roxmltree`, `zip` | GitHub's release feed |

**wgpu lockstep.** egui-wgpu and the engine use one wgpu version. When egui moves to a new wgpu major, Velo moves with it in one change.

## 7. UI toolkit: egui

egui with Velo's own kit. Immediate mode suits a UI that repaints every frame anyway (the preview, meters, the beat), and Ravune's theme and widgets already run on it. iced, Slint, GPUI, Makepad, Vello/Xilem and a Tauri web UI were compared: each either couldn't show the engine's picture cheaply, lagged behind wgpu, or meant a second language for the UI.

Conditions: no egui default look anywhere; colors, fonts and sizes only from the theme (CI checks for color literals); the specimen board (command palette → Specimen board) shows every widget in its states and what the UI costs per frame.

## 8. Reuse from Ravune

Ravune, the sibling DAW (`g1lg1l/ravune`), runs on the same stack. **Policy (D15): copy, adapt, don't share yet.** Modules are copied into Velo, trimmed, and keep a `From Ravune <commit>` line: the core model and operators, the app shell, kit, widgets, theme, icons and fonts (`scripts/fonts.py`), keymaps, i18n, files and autosave, MIDI, updates, the agents' server and the MCP bridge, the CI and release workflows, the Windows installer and `deny.toml`. A shared crate is extracted only when both apps have changed the same code twice. Nothing specific to a DAW is reused.

## Decision log

| # | Decision | Alternatives | Why | Revisit when |
|---|---|---|---|---|
| D1 | Rust, wgpu, one process, several threads | C++ (TouchDesigner, Resolume), a separate engine process | Safety and performance in one language | A UI crash kills a show in practice |
| D2 | egui with Velo's own design kit | iced, Slint, GPUI, Makepad, Tauri | §7 | eframe's per-frame cost becomes a problem |
| D3 | A render thread presents the outputs | Rendering on the main thread | The UI must never stall the output (spike S1) | - |
| D4 | Two data types: texture and signal | TouchDesigner's six families | Fewer concepts to learn | Geometry becomes a real need |
| D5 | Modulation as mappings on parameters | Control wires | Readable, fast to make | - |
| D7 | Shader nodes are WGSL with a JSON header; the built-ins use the same format | GLSL first, hard-coded effects | wgpu-native, validated by naga; every built-in tests the format | naga's GLSL front end becomes excellent |
| D8 | A project is a folder bundle with RON, stable ids and migrations | JSON, binary, one file | Shaders stay real files for any editor; readable diffs | Outside tools need JSON (add an export) |
| D9 | No native dylib plugins: shaders, built-in Rust nodes, outside tools over OSC and MCP | A dylib plugin API, FFGL | No stable ABI; a crashing plugin would take the show down | CPU plugins become a need: WASM components |
| D11 | The libraries are MIT OR Apache-2.0; the app is GPL-3.0-or-later because it ships Ableton Link (Link is close to essential in a booth, and Ableton's SDK is GPL unless licensed). NDI's runtime is loaded at run time, never bundled | Everything permissive without Link; Ableton's proprietary licence | Ecosystem convention for the libraries; Link without asking anyone | - |
| D12 | `Rgba16Float` working textures, display-referred values, no implicit sRGB conversion | Linear light, 8-bit | Feedback without banding; what shader authors expect | HDR outputs |
| D13 | HAP first for video, then the system's decoders | FFmpeg | HAP frames go to the GPU as they are; no codec licensing | Video on Linux |
| D14 | The computer's own audio (loopback) is a first-class source and the first-run default | The default input (a mic) | Trying Velo takes seconds: play something and it reacts. DJs with an interface pick it once, remembered per machine | - |
| D15 | Copy Ravune's modules into Velo; extract a shared crate only after real duplication | A shared crate now; starting from scratch | Fastest path, same stack and licence, no early API boundaries | Both apps change the same module twice |
| D16 | Separate GPU devices for the engine (render thread) and the UI (eframe); the preview by async readback; fullscreen is a stage window (borderless, display-sized, on top) | One shared device with a zero-copy preview; the OS fullscreen | With a shared device wgpu 30 fails surface reconfigures while the other thread submits, and eframe reconfigures on every resize (spike S1). Two devices also isolate GPU faults. macOS fullscreen is a Space with an animation, on the wrong display, hiding the UI | Readback cost matters: the render thread paints the UI too |
| D17 | The node canvas is the main view: one graph per scene, Velo's own canvas widget (Figma's view, TouchDesigner's Tab menu), panels around it | egui-snarl; a layer stack as the main view | egui-snarl lagged behind egui, and the canvas Velo needs (Figma's zoom, append on Tab, its undo and look) is one widget for the whole plane, cheap at 200 nodes (spike S4) | Graphs of thousands of nodes |
| D18 | In memory a Shader node holds its WGSL (`Node::code`, `node.set_code`): it rebuilds as you type and keeps its last good program, else passes its input. On disk a bundle keeps it in `shaders/` (`Node::file`), watched for outside saves | Code only in files, read by the engine; compiling on the UI thread | Undo, the AI and the build see one document; files make it hackable, and any editor works. Each outside save is one undoable `node.set_code` | Shaders shared between scenes: one file for several nodes |
| D19 | The AI runs as the user's own agent CLI (Claude Code, Codex, with only Velo's MCP bridge as tools) or as a model behind an OpenAI-compatible endpoint driven by Velo's own loop; it acts only through operators over MCP. API keys, for cloud endpoints only, live in the OS keychain, never in Velo's files | Keys in Velo's settings; giving the model Velo's internals | The user's own subscription and sign-in for the CLIs, nothing secret on disk; one surface (the MCP tools) for the panel and every outside client; every AI edit undoable and kept off the live scene | A provider with neither a CLI nor an OpenAI-compatible API |
