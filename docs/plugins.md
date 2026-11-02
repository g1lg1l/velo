# Velo: Shaders and Extensions

How to add your own visuals to Velo, and how outside tools drive it.

## 1. How Velo is extended

| Way | What | Language | Safety |
|---|---|---|---|
| **Shader nodes** | Generators and effects, written in Velo or any editor | WGSL; ISF (GLSL) imported | Validated by naga; a broken shader never reaches the output |
| **Built-in nodes** | Anything native (media, feedback, particles, signals) | Rust, in `velo-engine` | Code review |
| **Outside tools** | Remote control, live coding, AI agents, hardware bridges | Anything that speaks OSC or MCP | A separate process: it can't crash the show |

The built-in generators and effects are shader files in the same format as yours (`crates/velo-engine/shaders/`), so the format is tested by every one of them. Ready-made shaders to start from are in `presets.rs` (Shader node) and `effects.rs` (Shader FX).

No native dylib plugins: Rust has no stable ABI, wgpu objects can't cross a dylib built against another wgpu, and a crashing plugin would take the show down. No FFGL: it is OpenGL. Not yet: Luau script nodes, packages of shaders and scenes.

## 2. Shader node format

One `.wgsl` file. A leading JSON comment declares the node, in the spirit of ISF; the body defines `main_image`. Add a **Shader** (a generator) or a **Shader FX** (an effect) from the node menu and it opens with working code in the editor; every keystroke rebuilds it.

```wgsl
/*{
  "name": "Tunnel",
  "category": "generator",
  "description": "A tunnel that breathes with the bass and flashes on the kick",
  "params": [
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 4.0 },
    { "id": "rings", "type": "float", "default": 8.0, "min": 1.0, "max": 32.0 },
    { "id": "tint",  "type": "color", "default": [0.55, 0.35, 1.0, 1.0] },
    { "id": "style", "type": "int", "default": 0, "names": ["Stripes", "Rings", "Dots"] }
  ]
}*/

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let depth = 0.2 / max(length(p), 0.001) + g.time * speed;
    let bands = 0.5 + 0.5 * sin(depth * rings + atan2(p.y, p.x) * 3.0);
    let glow = bands * (0.4 + g.bass) + 0.5 * g.kick;
    return vec4f(tint.rgb * glow * smoothstep(0.0, 0.3, length(p)), 1.0);
}
```

`uv` runs 0..1 across the picture from the top left; `px` counts pixels.

### The header

| Key | Meaning |
|---|---|
| `name` | Required |
| `category` | `"generator"` (a picture from nothing) or `"effect"` (changes its input: needs at least one) |
| `description` | A line on what it does |
| `params` | Each `{ "id", "type", "default", "min", "max" }`; an `int` may have `"names"` |
| `inputs` | Textures, each `{ "id": "src", "type": "texture" }`, in binding order |
| `feedback` | `true`: the node reads its own previous frame as `prev` |
| `audio` | `true`: the node reads the audio texture as `wave` |
| `amount` | For effects: `"manual"` (the shader uses `amount` itself) or `"none"` (no amount knob) |

Unknown keys are an error, so a typo doesn't pass silently.

### Parameter types
`float`, `int`, `bool`, `color` (RGBA), `vec2`, `vec4`. An `int` with `"names"` counts from 0 to the last name and shows as choices. Floats, ints without names, `vec2` axes and colors can be mapped to a source like any built-in parameter.

### What Velo writes around the body
| Name | Content |
|---|---|
| `g` | The globals: `resolution` (vec2), `time`, `delta`, `frame`, `aspect`, `bpm`, `beat_phase`, `bar_phase`, `level`, `bass`, `mid`, `high`, `kick`, `snare`, `hat` (0..1, the hits as envelopes), `bass_time`, `mid_time`, `high_time`, `level_time` (clocks that run faster when a band is busy), `melody`, `beats` (beats counted so far) |
| `<param id>` | Each parameter as a plain variable named by its id: write `speed`, not `params.speed` |
| `<input id>` | One `texture_2d<f32>` per input |
| `smp` | The sampler: `textureSample(src, smp, uv)` |
| `prev` | The node's previous frame, with `"feedback": true` |
| `wave` | The audio texture, with `"audio": true` |

The ids `g`, `smp`, `prev`, `wave`, `uv`, `main_image`, `velo_params` and anything starting with `velo_` are reserved. Errors are reported at the line of your file, not of the wrapped one.

**Generators** get an `opacity` parameter (0..1) unless they declare one. **Effects** get `amount` (0..1, default 1): Velo returns `mix(input, main_image(...), amount)`, so 0 passes the input through. A shader that can do better (a blur whose radius scales with the amount) declares `"amount": "manual"` and must then return its input unchanged at 0.

### The audio texture
`wave` is 512 × 3 texels of `Rgba32Float`, uploaded once a frame when a graph uses it. Read it with `textureLoad`, without filtering:
- Row 0: the last 512 analysis hops (about 2.7 s at 48 kHz) as `(level, low, mid, high)`, 0..1, oldest to newest. `textureLoad(wave, vec2i(column, 0), 0)`.
- Row 1: 512 raw mono samples in R (-1..1), starting where the wave last rose through zero, so a steady tone stands still in a scope. `textureLoad(wave, vec2i(column, 1), 0).r`.
- Row 2: the spectrum in R, 512 bands from 30 Hz to 16 kHz evenly in octaves, 0..1, tilted up 4.5 dB an octave, rising at once and falling slowly. `textureLoad(wave, vec2i(column, 2), 0).r`.

The Waveform node draws all three (Spectrum, Radial, Scope, Rings).

### Particles
A file whose header says `"particles": true` has no `main_image`: it defines `velo_emit` and `velo_step` (compute) and `velo_vs`, `velo_fs` (a sprite per particle), and calls `velo_load()` to read its parameters. Only the built-in Particles node uses it for now (`crates/velo-engine/shaders/particles.wgsl`; [rendering.md](rendering.md) §6).

Not yet: several passes with named targets, shaders as scene transitions, compute nodes of your own.

## 3. ISF shaders

An ISF file (isf.video: GLSL under a JSON header of inputs, the format VDMX, Resolume and Synesthesia read) becomes a Shader node: Nodes → **Import an ISF shader** (Cmd+Shift+I), or drop a `.fs`, `.frag` or `.glsl` file on the window.

`crates/velo-engine/src/isf.rs` wraps the file in GLSL 4.50 bound the way Velo binds (globals, parameters, the sampler, textures from binding 3) and defines ISF's names: `TIME`, `TIMEDELTA`, `FRAMEINDEX`, `RENDERSIZE`, `isf_FragNormCoord`, `gl_FragCoord`, `gl_FragColor`, `IMG_NORM_PIXEL`, `IMG_PIXEL`, `IMG_THIS_PIXEL`, `IMG_THIS_NORM_PIXEL`, `IMG_SIZE`, `texture2D` (ISF counts from the bottom left, Velo from the top left). naga translates it to WGSL and Velo adds its vertex stage.

Inputs become parameters: `float`, `bool`, `event`, `long` (its `VALUES` as the range; its `LABELS` as choices when the values count from 0), `color`, `point2D`. An `image` input makes it an effect, with Velo's amount. The code stays GLSL in the project (a bundle keeps it as `shaders/<name>.fs`), editable in Velo or any editor; mistakes come back at the author's lines.

Not yet: several passes and persistent buffers, more than one image input, `audio` and `audioFFT` inputs, ISF's own vertex shaders.

## 4. Outside tools

- **OSC** in, on a UDP port (9000 by default, off until Preferences → OSC), with an address space under `/velo` that follows the project: macros, scene launches, variations, parameters, blackout, freeze, any operator. TouchOSC on a phone, a lighting desk or a live-coding setup works with no Velo-specific code. See [control.md](control.md).
- **MCP**: with Preferences → AI and agents → Let agents use Velo, Velo runs a JSON-lines server on localhost (a secret in Velo's data folder keeps web pages out), and `velo-mcp`, the stdio bridge installed next to Velo, speaks MCP to any client (Claude Code, Codex, Claude Desktop, Cursor). Ten tools: `list_nodes`, `query_project`, `search_operators`, `run_operator`, `snapshot`, `shader_errors`, `listen`, `checkpoint`, `undo`, `save_project`. Agents always read; the user decides whether they may edit, touch the live scene or save. Everything an agent changes until its next checkpoint undoes in one step, and every call is logged.
