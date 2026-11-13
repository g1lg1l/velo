# Velo: Node Graph

How a scene is built: nodes, wires, mappings, and what the render thread does with them each frame.

## 1. Decisions

| Question | Decision | Why |
|---|---|---|
| How many data types? | **Two: pictures (textures) and signals (numbers)** | Live 2D visuals need images and numbers. Particles, text and raymarched shapes live inside nodes and output pictures. |
| How does a signal reach a parameter? | **A mapping on the parameter**: drag a source onto a slider, or wire a signal output onto a card | Per-parameter modulation (as in Bitwig or Serum) reads better than control wires everywhere. |
| What is a scene? | **One graph**, wired by hand on the node canvas | One patch, Tab to add. The Layer Stack node composites up to eight inputs for anyone who thinks in layers. |
| Cycles? | **Only through Feedback** (it reads its own last frame) | Evaluation stays a plain topological sort. |
| How are built-ins written? | **As shader files in the format users write** | The extension format is tested by every built-in. |
| Effect strength? | **Every effect has `amount` 0..1; 0 passes its input through** | Any effect is fader-mappable and crossfadable, and bypass is `amount = 0`. |
| Picture strength? | **Every generator (but Particles) gets `opacity` 0..1**: Velo multiplies what it draws, so 0 is gone; one that declares its own (Text) keeps it | Any picture fades the same way. |
| When does the graph change? | **Compiled off the render thread, swapped at a frame boundary** | A structural edit never stalls a frame. |
| Signals every scene reads? | **One shared signal graph per project**, run once a frame, its outputs mapped from any scene (`shared:n_ab12/0`) | A show that reads the music one way everywhere keeps its composite kick or drop detector in one place: one threshold changed, every scene follows. |

## 2. Structure

A project is a list of scenes and one shared signal graph (§3, Shared signals); a scene is `Scene { nodes, edges, launch, macros, variations, ties }` in `velo-core` (macros, variations and launching are in [scenes.md](scenes.md)). The node kinds are `NODE_KINDS`: static data, so the UI, files and tests reason about graphs without a GPU.

- A **node** has a stable id (`n_ab12`), a kind (`velo.noise`), a name, a position, the parameters the user changed (the rest keep the shader's defaults), its mappings, and, by kind, its code (Shader, Shader FX), its file (`media`) or its words (`text`). Unknown kinds from a newer file load and keep their wires.
- An **edge** goes from an output to an input, each `(node id, port)`. An input takes one wire.
- A **tie** gathers cables into a bundle where it sits on the canvas.
- A node can be turned off (power button, context menu, `E`): an off node passes its first input through (black if it has none), an off signal node gives nothing. Output nodes stay on.

## 3. Ports, parameters, sources

### Ports (wires)

| Type | Lives on | Example |
|---|---|---|
| Picture | GPU, `Rgba16Float`, the project's canvas size | a generator's output, an effect's input |
| Signal | CPU, one `f32` per frame | an LFO's output, an Audio In's kick |

Pictures go into picture inputs and signals into signal inputs; the one exception is the Probe, whose input takes a picture (`Scene::can_connect`). A signal output dropped onto a picture node becomes a mapping of one of its parameters.

### Parameters (knobs)

Declared in each shader's header: `float`, `int`, `bool`, `color`, `vec2` (and `vec4` for custom inspectors). Floats, ints, vec2s (each axis, `offset.x`) and colors are mappable; on/offs are not. A value's right-click menu resets it to its default.

Nothing moves unless asked: built-in nodes start still, and the `audio` knob of shader presets and the `music` knob of effects (both shown as **Automagic**, how much the node plays along by itself) start at 0. Motion is a parameter you set or a signal you map, and both show.

### Mappings

```rust
pub struct Mapping {
    pub param: String,    // "zoom", or a vec2's axis "offset.x"
    pub source: String,   // a signal node's output: the scene's ("n_ab12/4") or a shared one's ("shared:n_cd34/0")
    pub amount: f32,      // -1..1 of the parameter's range
    pub attack_ms: f32,   // one-pole smoothing, up and down
    pub release_ms: f32,
    pub invert: bool,     // read upside down: 1 in silence, 0 on a hit
}
```

Edited with `mapping.add` (half the range, 5 ms attack, 150 ms release), `mapping.set` and `mapping.remove`, all undoable. Smoothing lives on the mapping, not the source: one Kick can punch the zoom and nudge the hue softly. Each frame:

```
value = clamp(base + Σ amount × (max - min) × smoothed(source), min, max)
```

A mapped color turns round its hue instead: `amount` × the source's level, in turns, so a Time signal at amount 1 cycles a tint once a period.

### Sources

`velo_core::SOURCES`: `level`, `bass`, `mid`, `high`, `kick`, `snare`, `hat`, `beat`, `bar_phase`, `melody`. Each is the output of a signal node: a shared source given to `mapping.add` (dragged from the deck or named by an agent) is wired from the scene's Audio In or Beat, which is added if the scene has none. Any other signal node's output is named `node id/output` (`n_ab12/1`), and an output of the shared signals `shared:node id/output` (`shared:n_cd34/0`; `shared:n_cd34` is its first).

### Shared signals

A project has one **shared signal graph** (`Project::shared`): signal nodes and their wires, with positions, groups and notes like a scene's canvas, and nothing else (no pictures, no Output, no Probe: there is no picture to read). It runs once a frame for the whole show, and every scene's mappings can read its outputs, whichever scenes render: the live one, one fading out, the layers, the edited one. A show that reads the music the same way everywhere (a composite kick, a pulse, an energy follower, a drop detector) keeps those nodes there once, instead of a copy in each scene.

- **Edited** on the node canvas like a scene: the **Shared signals** entry at the top of the scenes rail opens it (it is no scene: it can't be launched, made a layer, put in the autopilot, renamed or deleted, and has no macros). The node menu offers signal nodes only.
- **Mapped** like any source: its outputs are on the deck's SOURCES (marked with a broadcast sign), to drag onto any slider of any scene. A card fed by one has a jack for it (`📡 Pump → Zoom`) and glows with it, with no line on the canvas, since the node lives on the shared canvas; a shared node's inspector lists what it moves in every scene, and changes it from there.
- **Operators**: the node, wire, tie, group and note operators take `scene: "shared"` (and `scene.select` with `"shared"` makes it the edited graph, which they then default to); `node.add` refuses anything but a signal node there. Scene operators (rename, launch settings, layers, the autopilot, macros, variations) refuse it. `mapping.add` takes `"shared:n_cd34/0"` from any scene; nothing in the shared graph takes a mapping.
- **Deleted** shared node: the mappings that read it stay (an undo brings it back whole) and read 0, so their parameters sit at their base values.
- **Ids**: node ids are unique across the scenes and the shared graph, so a binding or OSC address naming a node finds a shared one too.

### Signal nodes

Signal wires never mix with picture wires. Signal nodes run on the render thread every frame, in wiring order, into up to 64 slots per scene after the audio and clock sources; the shared signals run first, into 64 slots of their own after those, which no scene's pass writes over. The inspector and the cards read the edited scene's values back, and the shared signals' every frame.

| Node | Outputs | What it does |
|---|---|---|
| Audio In (`velo.audio`) | level, bass, mid, high, kick, snare, hat, melody | what the audio input hears |
| Beat (`velo.clock`) | beat, bar | a hit on every beat, the ramp through the bar |
| Band (`velo.band`) | out | how loud the music is from `low_hz` to `high_hz` (300 to 1500 Hz to start with): the mean of the audio texture's spectrum row there (each band 0..1 over 60 dB, as the Waveform draws it), with `auto` (on to start with) stretched to the range it has moved in lately as a Remap's, falling over `release` (150 ms). The spectrum is 512 points, about 94 Hz a bin at 48 kHz: under 150 Hz edges blur (30 to 60 Hz reads the lowest bins, a sub and a kick's body together) |
| Beat Kick (`velo.beatkick`) | out | on each hit of `trigger` (the beat while nothing's wired) a 5 ms rise and a fall over `release` (20 ms to 2 s, dropping fast then tailing off), times `in` (the bass while nothing's wired); 0 while `in` is under `threshold` |
| LFO (`velo.lfo`) | out | sine, triangle, saw, square or random on the beat clock, 1/4 to 64 beats, phase offset |
| Time (`velo.time`) | out | 0 to 1 over 0.5 to 600 seconds, again and again: a ramp, back and forth or a wave |
| Random (`velo.random`) | out | a new value every 1/4 to 64 beats, or on each hit of `trigger`; `glide` eases there |
| Probe (`velo.probe`) | out | a picture read as a number (below) |
| Remap (`velo.remap`) | out | its input into a range: auto or fixed, curve, gate, attack and release, invert |
| Math (`velo.math`) | out | a and b into one: add, subtract, multiply, min, max, average, difference |
| Gain (`velo.gain`) | out | its input times `gain` (0 to 16, 2 to start with), kept within 0 and 1: what makes a faint signal strong, which a Math's `value` (up to 1) can't |
| Envelope (`velo.envelope`) | out | on each hit, up over the attack and down over the release, through a curve |
| Counter (`velo.counter`) | out | a step on each hit through 2 to 16 values: up, down, ping-pong or random; `reset` |
| Sequencer (`velo.sequencer`) | out | 2 to 16 steps of a quarter beat to a bar each, a pattern of its own |
| Sample & Hold (`velo.hold`) | out | on each hit of `trigger`, `in`'s value (or a random one) until the next |
| Toggle (`velo.toggle`) | out | 0 or 1, flipped on each hit |
| Compare (`velo.compare`) | out | 1 while a is above (or below) b or a fixed value, else 0 |
| Timer (`velo.timer`) | out | a hit every so many seconds; `reset` starts it over |
| Smooth (`velo.smooth`) | out | its input followed: Glide at a steady speed, or Lag (one-pole), rise and fall apart |
| Quantize (`velo.quantize`) | out | its input snapped to 2 to 32 even steps |
| Speed (`velo.speed`) | out | round a cycle of 0.1 to 600 seconds as fast as `in` says (full while nothing's wired), as a ramp, back and forth or a wave: a louder input turns it faster instead of further; `reset` starts it over |
| Delay (`velo.delay`) | out | its input as it was 1/4 to 16 beats ago on the clock |
| Spring (`velo.spring`) | out | its input followed on a spring: a swing of 50 ms to 4 s, `bounce` how far it overshoots and how long it rings |
| Slope (`velo.slope`) | out | how fast its input rises (or falls, or either): 1 when it crosses the whole range in `time` (10 ms to 2 s) |

A card with several outputs (Audio In, Beat) lists them as rows, each with its meter and port; its inspector picks which rows show. Any other signal node's card says what it does in a line with its settings (`trigger × in · silent under 0.30 · falls in 260 ms`, `a − b`, `in × 2.0`, `1 while a is below 0.25`), draws what comes in as thin grey lines, the level they must cross as a dashed one, and over them what it gives as a green curve running to its port, all over the last second and a half; a Sequencer's steps and a Counter's places are a row of squares (an empty step an outline, a set one filled, the one it's on bright); its foot names each input with what comes in, and what goes out at the right. Far out, the curves alone. Every such small graph, an Audio In's and a Beat's rows too, is drawn on a grid: lines at 0, a half and 1 with "1" and "0" at its top and bottom, and as many upright lines as keep its cells near square. A signal node's inspector also lists what its outputs move, with the same settings as the moved parameter's mapping chip, so a link is changed from either end.

**Probe.** After the frame is drawn, `probe.wgsl` draws the region (`size` of the picture round `position`; the whole picture by default, a point at size 0) into 32 x 16 cells of 4 x 4 taps each, which are read back and averaged; the next signal pass measures that color (brightness, red, green, blue or alpha; see-through counts as dark). One read is in flight at a time, so it lags the picture by a frame or two.

Recipes: Noise into Threshold into a Mask is a moving stencil; an LFO through Quantize into a Switch's value steps through its pictures; a Kick through Smooth is a slow swell; a Melody through a Gain of 4 fills a range it barely reached; a Beat Kick onto a Transform's scale pumps on the grid through the drop and rests in the breakdown; Bass into Speed onto a Transform's rotate (set to -1, amount 1: a whole turn) turns faster with the bass and never jumps back; a Kick through a Delay of half a beat is its echo; a Video into a Time Machine with a Gradient as its map is a slit-scan; a Video into a Probe onto a Blur's radius.

## 4. Node kinds

Every picture node is a shader file in `crates/velo-engine/shaders/`; signal nodes are Rust (`velo-engine/src/signal.rs`). The node menu (`Tab`) groups them as below, with the signal nodes split into Signals (Audio In, Beat, Beat Kick, Band, LFO, Time, Random, Probe) and Logic (the rest).

| Group | Nodes |
|---|---|
| Generators | **Noise** (value, Perlin, simplex, Worley, cell edges, mosaic; ridged or turbulent, warped, evolving) · **Gradient** (linear, radial, angular, diamond; repeated, mirrored, scrolling) · **Shapes** (15 shapes in rings, outlined or filled, glowing, twisted, dashed, tiled) · **Waveform** (the spectrum as bars or a ring, a scope, beat rings) · **Shader** (your own WGSL or ISF, or one of 76 presets from Plasma to V Beams) · **Image** (a still, a GIF, a folder of frames or a HAP movie; a photo shows upright, turned as its EXIF orientation says; HEIC and HEIF on macOS only, where the system decodes them; fill, fit or stretch; speed, loop points, direction, cue and scrub: Playing a file, below) · **Video** (HAP; H.264, HEVC and ProRes through AVFoundation on macOS, H.264 and HEVC through Media Foundation on Windows; a movie shot with the phone turned plays upright, at the size it shows, on macOS, and as it's stored on Windows for now) · **Live** (an NDI source, a Spout sender or a camera, as it comes; or, in a layer on top, what plays under the layer, a frame late, for an effect over everything: [scenes.md](scenes.md) §3) · **Media List** (up to 256 files in turn, by beats or seconds, or on Hits: the next file each time its Hit parameter rises past a half, so a Kick mapped onto Hit changes the picture on every kick and nothing changes while there are none; by beats or seconds a movie's turn as long as Movie every says (at 0, as a picture's; one shorter than its turn moves on once played through, or repeats, as Short movie says), cut or crossfaded (on Hits the fade takes its share of Every beats, and a movie repeats until the next hit); streamed from disk, only the turn before, this one and the next in memory, or all loaded in memory so no turn waits) · **Text** (words in Inter, JetBrains Mono or a font file: size, tracking, outline, shadow, box, reveal) · **Text List** (items one at a time, typed in or from a .txt or .csv; at random it deals them from the deck the Media List deals its files from, shuffled on the CPU: every one once before any comes again, a new shuffle each round, never the same twice running) · **Particles** (up to 65,536 sprites on the GPU from an emitter; emission starts at 0, map Kick onto it) · **Mesh** (a 3D model, an .obj or .ply, drawn solid and lit from a camera turning round it; the picture wired into `skin`, a photo, a movie, a camera or any node, covers it under the light, by the model's own texture coordinates or, without them, laid on from its six sides) |
| Effects | **Transform** · **Kaleido** (both with an optional `map`: its brightness scales the move or the fold pixel by pixel, as far as `modulate` says; at 0, or with nothing or a see-through picture wired, they move as they always did) · **Color** · **Blur** · **Feedback** (its last frame zoomed, turned and fading under the input; zoomed and turned about `center`, the zoom curving with the distance from it, `zoom curve` (MilkDrop's zoomexp: over 1 the edges zoom more than the middle), and pushed about by a slow `warp` of four sine waves, MilkDrop's, whose `warp speed` and `warp scale` set how fast they move and how big their swirls are; `warp` goes up to MilkDrop's usual amount and starts at 0) · **Displace** · **Opacity** · **Levels** · **Threshold** · **Edges** · **Channels** · **Crop** · **Polar** · **UV Map** (a second picture says where each pixel reads) · **Lookup** (recolored from a second picture, its palette) · **Freeze** (held while `hold` is over a half, or a new frame every few beats) · **Time Machine** (its input up to 31 frames ago, kept at half size; with a map, each part as far back as the map is bright) · **Key** (a color, the dark or the bright cut out, spill taken off the edges) · **Luma Blur** (blurred as much as a second picture, its map, is bright) · **Shader FX** (your own WGSL on its input, or one of 41 effects from Glitch to Solitaire) |
| Composite | **Blend** (up to 16 inputs, 14 modes, intensity, a key) · **Mask** (keep, cut or multiply) · **Merge** (up to four pictures as one picture's channels) |
| Control | **Switch** (one of up to sixteen inputs as `value` picks, cut, crossfaded, dissolved, wiped or through a circle; only what it shows renders) · **Grid** (up to 4 x 4 cells of eight inputs) · **Layer Stack** (up to eight inputs with blend mode, opacity and order; a signal on a layer's order lifts it) |
| Output | **Output**: what goes out, through its grade (exposure, contrast, saturation, hue, colorize, invert, vignette, fade to black), on the preview, the displays and the shares alike. With several, the last one something is wired into goes out; at its defaults the picture passes unchanged. |
| Signals, Logic | the 24 signal nodes in §3 |

**Playing a file.** An Image or Video node with frames (a GIF, a folder of frames, a movie) plays them on a clock of its own at `speed` (0 to 4, 0 holds), looping the part between its loop points, which the inspector shows as the file's timeline. `direction` plays that part forward, in reverse (from its end back) or ping-pong (forward, then back). `cue` starts playback over each time it rises past a half, as a Media List's Hit does: a Kick mapped onto it cuts the clip back on every kick, a key or a pad bound to it cues by hand. `scrub` (0 to 1 of the loop) is where playback runs from, added to the clock: a cue lands there, and with `speed` at 0 since the graph was built or since the last cue it is the playhead itself, so a fader or an LFO mapped onto it moves through the loop by hand. Nothing hands over: with speed above 0 the clock plays on from wherever scrub puts it, and moving scrub moves the picture by as much. Reverse, ping-pong and scrubbing are smooth on GIFs, frame folders and HAP, whose frames read in any order; a system-decoded movie (H.264, HEVC, ProRes) opens its reader again at each frame going back, a few milliseconds for ProRes, whose frames stand alone, but long enough on a long-GOP H.264 or HEVC to stutter: transcode to HAP to play those backwards. Defaults (forward, no cue, scrub 0) play as before.

Several pictures or movies dropped on the canvas at once become one Media List (a new one every 4 beats), wired into an Output nothing feeds yet, in one undo step. A .txt, .csv or .tsv dropped becomes a Text List the same way: a line an item (a block between empty lines, if there are any), or a row an item with its cells on lines of their own, up to 256; its inspector's From a file… replaces a list's items with a file's. Excel's CSV comes in with its semicolons and accents. Freeze and Feedback read their own last frame. A Shader node starts from the first preset (Tunnel), a Shader FX from Glitch; Custom, the template to start code of your own from, heads their preset menus. Saved Shader and Shader FX nodes keep their code; a built-in node changes with its shader.

`VELO_SHOTS=<dir> cargo test -p velo-engine --release contact_sheets -- --ignored` draws every built-in, preset and effect into contact sheets (four frames each, one on a kick with Automagic up, and the cost of a 1080p frame).

Not yet: a Delay node (frames from the past, slit scans) needs a ring of past frames per node (a second of 1080p in `Rgba16Float` is about 1 GB), so it waits for half-size frames or a few taps.

## 5. Evaluation per frame

On the render thread (`velo_engine::runtime`), for the live scene, a scene fading out and the edited scene, each once:

```
1. commands          new graphs, base values, mappings, macros, launches, taps, the shared signals' plan (a channel)
2. pace and launch   the output's vsync (or a 60 Hz tick); a waiting launch lands on its boundary
3. read sources      the latest audio features, the beat clock; then the shared signal graph, once
4. signal pass       each graph's signal nodes in wiring order, Probes fed what they last read,
                     Bands the spectrum the audio thread last published
5. resolve params    macros set driven bases, mappings add on top, written to parameter buffers
6. texture pass      media frames due, every node's pass in topological order, Probe reads
7. outputs           the program (or a transition), the layers on it (what is under an effect
                     layer kept for its Live node, read at step 6 of the next frame), freeze,
                     blackout, test pattern; the output window, Spout or NDI, the preview and
                     the cards' pictures; submit, present
```

Steps 1 to 5 are CPU work measured in microseconds; step 6 is where the frame goes. MIDI and OSC are applied on the UI thread through operators, so they arrive in step 1.

The shared signals (`signal::plan_shared`) are sent whole when they're rewired (`Command::Shared`, their outputs' names with them) and retuned in place when a setting changes; each node keeps what it was doing across a rewire (a Smooth's level, a Beat Kick's fall, matched by node id), and the UI routes every mapping of them again in the same batch, to their outputs' new slots. Nothing allocates per frame: the sources are one array on the stack, the scenes' signals after the audio and clock, the shared ones after those, and a slot no node writes any more reads 0.

Steps 3 and 5 are `velo_engine::modulate`. The UI sends a node's base values (`Command::Params`) and its mappings resolved to indices (`Command::Mappings`), both without a rebuild. Each mapping keeps its own one-pole state in the engine, carried over when its settings change and reset by a rebuild. The inspected node's results come back through atomics for the slider's live dot; the UI never computes modulated values. The beat clock is a tempo and the time of beat 0, so phase is a function of time; taps and resyncs arrive carrying their `Instant`.

## 6. Compilation

`velo_engine::compile`: `plan` turns a scene into pure data (which shader each node runs, what feeds each input, which node goes out, what each Probe reads); the `Compiler` builds the programs and textures on a worker thread, so neither the UI nor the render thread compiles. The render thread swaps the new graph in at a frame boundary and drops the old one off its own thread.

- **Hot path:** a knob sends new values, no compile. Adding a node, rewiring or editing a shader rebuilds.
- **Programs** are cached per built-in shader. A Shader node whose new code fails keeps the last code that compiled, and its error comes back to the editor.
- **Media** (images, movies, fonts) loads on threads of its own; a node whose file is still loading, or can't load, shows black, and the graph is built again once the file is in.
