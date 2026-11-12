# Velo: Product and interface

What Velo is for, the rules it is built by, and how its interface works.

## 1. One line

**Sound shapes light.**

An audio-first instrument for live visuals: TouchDesigner and Resolume power, approachable in five minutes.

Velo turns sound into motion on a projector. Play some music, pick a visual, drag Kick onto a parameter, map a knob, send it fullscreen. Everything deeper (the node patch, your own shaders, an AI that builds scenes) is one deliberate step away.

## 2. Who it is for

| Who | Uses today | Needs from Velo |
|---|---|---|
| **DJ or producer** with no VJ background | Nothing, a YouTube loop, Synesthesia | Plug in, it reacts to the music, fullscreen on the club's projector. No patching. |
| **Working VJ** | Resolume, VDMX | Reliability, MIDI everywhere, scene changes on the beat, low latency, no dropped frames. |
| **Shader artist, creative coder** | Shadertoy, KodeLife, TouchDesigner, Hydra | Write a shader, see it at once, get its parameters as knobs, make it react to audio without boilerplate. |
| **Installation artist** (later) | TouchDesigner | Runs unattended: starts a project by itself, recovers from crashes and unplugged displays. |

The first two set the defaults, the third the extension model.

## 3. What it is, what it is not

**Is:** a realtime 2D visual instrument (GPU textures in, textures out) with audio analysis built in, live control (MIDI, keyboard, OSC), scenes with musical transitions, and output to displays, Spout and NDI.

**Is not:**
- **A 3D engine.** No scene graph, textures, shadows, animation or PBR. 3D looks come from shaders (raymarching), from Particles (a camera turning round a sphere or the points of a model) and from the Mesh node: one model (.obj, .ply) drawn solid and lit by a light. Notch and TouchDesigner own the rest of this space.
- **A projection-mapping suite.** The output keeps the picture's shape on any display; mesh warping and edge blending are for MadMapper.
- **A general programming environment.** No tables, no Python, no DATs. Logic beyond modulation goes to outside tools over OSC or MCP.
- **A video editor or timeline sequencer.** Shows are played live (scenes, macros, variations), not rendered from a timeline.
- **A lighting controller.** No DMX or Art-Net.
- **A host for legacy plugins.** No FFGL (OpenGL), no Quartz Composer.

## 4. Principles

Ranked: when two conflict, the higher one wins.

1. **Never drop a frame on stage.** Live safety beats features. Anything that can stall (dialogs, shader compiles, file loads, the UI) runs off the render thread. A broken shader keeps the last good one on the output.
2. **Sound in, motion out, in seconds.** The analysis is always on and its results are one drag from any parameter.
3. **Progressive disclosure.** Play, then Patch, then Code (§7.5). Each level is opened on purpose, never pushed on a beginner.
4. **One concept per job.** Pictures are *textures*, control data is *signals*, a show is *scenes*.
5. **Small.** Small binary, small memory, few dependencies, few concepts. Features earn their place.
6. **Hackable.** Plain-text projects, shaders as files that reload on save, every action an operator reachable from the keyboard, the command palette, OSC and MCP.
7. **Beautiful but cheap.** Motion that carries information (meters, the beat, learn mode), never decoration that costs frames.

## 5. Budgets

Targets, measured on the reference machines.

| Budget | Target | Why |
|---|---|---|
| Cold start to first output frame | < 1 s | A restart after a crash should barely show |
| Idle CPU, simple scene, 1080p60, M1 | < 5 % of one core (render, analysis, UI) | Laptops on battery, thermal headroom |
| GPU time, typical scene, 1080p, M1 / Intel Iris Xe | < 4 ms / < 8 ms | Room for 60 fps with outputs and UI |
| Audio to engine latency (display excluded) | ≤ 15 ms at 48 kHz, 256-frame buffers | The kick must look like it hits with the kick |
| Scene switch | 0 dropped frames | Principle 1: a scene is built before it launches |
| UI stall | The output keeps 60 fps while the UI thread is blocked for 2 s | Principle 1 |
| Release download, per platform | < 25 MB | Principle 5 |
| Resident memory, empty project | < 150 MB | Principle 5 |

## 6. Vocabulary

| Word | Meaning |
|---|---|
| **Project** | One show: a `Show.velo/` folder with `project.ron` and the shaders' files. |
| **Scene** | One look you can launch ("Intro", "Build", "Drop"): a graph of nodes, eight macros and up to eight variations. |
| **Node** | One step of the picture or of a signal: a generator, an effect, a composite, a control, a signal, the Output. |
| **Source** | Anything that moves and can drive a parameter: Level, Bass, Mid, High, Kick, Snare, Hat, Melody, the beat clock, a signal node's output. |
| **Mapping** | "This source moves that parameter, this much, with this feel." It lives on the parameter. |
| **Macro** | One knob of a scene that moves several parameters, each over its own range. |
| **Output** | Where the picture goes: the preview, a window, a fullscreen display, Spout, NDI. |

## 7. Interface

### 7.1 Layout

```
+--------------------------------------------------------------------------------------------+
| Show.velo v | Computer audio  128.0 BPM | MIDI  Output  Freeze  Blackout |  search  undo  AI |
+-----------+------------------------------------------------+-------------+-----------------+
| SCENES    |                                    +---------+ | INSPECTOR   | AI              |
| 1 Velo    |  [Audio In]---->[Shader]---->[Output]  | preview | | the node's  | the chat, the   |
| 2 Build   |      cassette     V Beams              +---------+ | parameters  | model, pictures |
| 3 Drop    |                                                | and links   |                 |
|           |                node canvas                     |             |                 |
+-----------+------------------------------------------------+-------------+-----------------+
| VARIATIONS A B C ... | MACROS 1 to 8, numbered knobs        | SOURCES level meters, hit keys |
+--------------------------------------------------------------------------------------------+
| notices                                                    fps, frame time, CPU, memory, GPU |
+--------------------------------------------------------------------------------------------+
```

- **Top bar: the live state.** The project and its menu; the sound (the audio input with its state, the tempo with what leads it); the stage (MIDI, the Output menu, Freeze, Blackout, red while on). On the right: preferences, the AI, the panel toggles, where the preview shows, undo and redo, command search.
- **Scenes rail (left).** A card per scene: LIVE on the one going out, pulsing with the beat, a countdown ring while one waits to launch. Its + adds an empty scene or one from a template; the autopilot button is there too.
- **Node canvas (center).** The selected scene's graph on an endless plane. The preview floats over it as a card you can move; it can also sit behind the canvas, beside or under the nodes (the split canvas), or hide.
- **Inspector.** The selected node's parameters and what moves them; with nothing selected, the scene's launch settings. Docked on the right, beside the nodes when the canvas is split, or in a window of its own.
- **AI panel (outermost right).** Closed until asked for (`Cmd+I`).
- **The deck (bottom, the window's width).** What you play: the scene's variations, its eight macros, and every source as a live meter (levels as bars, hits as keys that flash). Drag a source onto any parameter to map it.
- **Status bar.** Notices on the left, Velo's load on the right.

Each panel slides away with its toggle or key; `\` (or `|`, or `Cmd+\`) hides them all.

### 7.2 The five-minute path

A new user, no manual, does these in five minutes.

| # | Step | What they do | What makes it quick |
|---|---|---|---|
| 1 | Open | Launch Velo | The first launch opens the demo, live, already reacting to whatever the computer plays. Press play anywhere: no cables, no setup. |
| 2 | Pick audio | The audio chip | **Computer audio** comes first; below it the input devices and, on macOS, single apps. Remembered per machine. |
| 3 | Add a visual | `Tab` on the canvas | The node menu: every node with its icon and a line on what it does, a tab per group. Type a few letters, Enter. With a node selected, the new one is wired after it. |
| 4 | Link to the music | Drag **Kick** from the deck onto a slider | A mapping with a sensible default (half the range, 5 ms attack, 150 ms release). The slider shows how far it reaches and where the value is now. |
| 5 | Map a knob | Right-click a control → Learn MIDI, turn a knob | Bound, with a `CC 21` badge. `L` toggles learn mode for many controls in a row. |
| 6 | Go fullscreen | Output → a display | Displays are listed by name. The output shows no UI and no cursor. |

### 7.3 Modulation

After Bitwig's modulators and Serum's drag-to-modulate.

- **Drag a source onto a slider** to map it. Several sources on one parameter add up.
- On the card, a signal plugs into a jack in its footer, one line per link saying where from and what it moves. Its chip opens the mapping's popover: amount (either way), attack, release, invert, remove. `Alt`+drag on the slider sets the amount.
- The slider shows the base value, how far the mapping reaches, and the live value, so you can see why something moves.
- Signal nodes (LFO, Envelope, Sequencer, Sample & Hold, Smooth, Math and more) make new sources from old ones.

### 7.4 Live control

- **MIDI learn** on everything that plays: right-click → Learn MIDI, or `L` for learn mode (control.md §3).
- **Macros.** Each scene has eight. Map the controller to the eight macros once; each scene decides what they do, so one controller layout works across the show. Variations store where the macros are; Shift+click morphs there over a bar.
- **Scene launch.** Click, number keys, MIDI pads, OSC. A launch waits for the next beat, bar or four bars, with Cut, Crossfade, Wipe, Zoom, Glitch or Flash. The autopilot launches the next scene by itself.
- **Panic controls.** Blackout (`B`), Freeze (`F`), and a test pattern (`Cmd+Shift+T`) for lining up a projector.

### 7.5 Progressive disclosure

| Level | Who | What shows | Opened by |
|---|---|---|---|
| 0 · **Play** | Anyone | Scenes, audio, the deck, Output, Blackout | The default |
| 1 · **Patch** | Most users | The node canvas, the inspector, mappings, signal nodes | The default view, used when wanted |
| 2 · **Code** | Shader artists | A Shader node's WGSL in the code editor with live errors, or in any editor | Adding a Shader node, or "Open in editor" |
| AI | Anyone | The AI panel, building scenes from a sentence | `Cmd+I` |

### 7.6 Keyboard

The stage keys: `1` to `9` launch scene n · `Space` (a tap) launches the selected scene · `0` auto (the autopilot on, every layer back on) · `D` drop (the autopilot's next scene now, through a flash) · `T` tap tempo · `B` blackout · `F` freeze · `P` show or hide the preview · `L` MIDI learn. They stay free of every other command.

A project can give a key another job: in MIDI learn a letter, a digit or `F1` to `F12` pressed alone binds to the control that waits, like a pad (control.md §2 and §3), so `4` can flip a macro and `7` a parameter. A key bound in the open project does that instead of its command here; the others keep theirs. Bound keys follow the stage keys' rule: nothing fires while a text field, the code editor or a dialog has the keyboard.

Everywhere: `Cmd+K` command palette · `Cmd+S` save · `Cmd+Shift+S` save as · `Cmd+O` open · `Cmd+N` new · `Cmd+Z` undo · `Cmd+Shift+Z` or `Cmd+Y` redo · `Cmd+I` the AI · `Cmd+Shift+H` frame-time HUD · `Cmd+Shift+T` test pattern · `Cmd+,` preferences · `F1` the shortcut sheet.

On the node canvas: `Tab` node menu · `H` or `Shift+1` zoom to fit · `Shift+2` zoom to selection · `Shift+0` 100 % · `Cmd+=` / `Cmd+-` zoom · `E` turn the selection off or on · `Cmd+D` duplicate · `Delete` delete · `Cmd+A` select all · `Cmd+Shift+I` import an ISF shader · `Esc` deselect · `Space`+drag pan. Bare keys work where the canvas has the keyboard or the pointer; chords whenever it shows.

Scenes: `Cmd+Shift+N` new · `F2` rename · `Cmd+Backspace` delete. Panels: `Cmd+B` scenes · `Cmd+Alt+B` inspector · `Cmd+J` the deck · `\` all of them · `Cmd+Shift+P` split the canvas · `Cmd+Shift+B` inspector in its own window.

`Cmd` is `Ctrl` on Windows and Linux. Every command can be rebound (`crates/velo/src/keymap.rs`).

### 7.7 Look

A calm, shadcn-like look for a dark room.

- **Themes:** Velo Dark (neutral greys, the default), Velo OLED (pure black) and Velo Light; a theme file can be imported. **One accent**, electric cyan; other colors only for state and for the node categories.
- 1-px borders instead of shadows; corner radius 6 px on controls, 10 px on cards; a 4-px grid.
- **Inter** for the interface, **JetBrains Mono** for every number, **Phosphor** icons, Noto Emoji for node and scene icons, all bundled.
- Motion: short ease-out fades and slides (about 160 ms), where it carries information: the beat on the tempo dot, meters, learn mode's outline, the launch countdown. **Reduce motion** in Preferences turns it off.
- Colors live only in the theme module (CI checks it).
- English and Italian.

Not yet: a stage theme with capped brightness, so the laptop doesn't light up the performer's face.

### 7.8 Safety rails

- `Esc` closes the output window only when that window has the keyboard; a stray `Esc` in the main window never does.
- A lost audio device is asked for again every second; the visuals keep running. A MIDI port that comes back is reconnected and its bindings work again.
- A shader error shows in the editor; the output keeps the last code that compiled.
- Unsaved work is snapshotted every 60 s; after a crash Velo offers to restore it.
