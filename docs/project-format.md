# Velo: Project Files

How a show is saved. The model is `velo-core` (`Project` in `lib.rs`); reading and writing is `crates/velo/src/files.rs`.

## 1. Decisions

| Question | Decision | Why |
|---|---|---|
| Container | **A folder bundle** `Show.velo/`; a single `.velo` RON file also opens and saves (older projects, the autosave) | Shaders stay real files for outside editors; folders diff and version well in git |
| Text format | **RON** (`project.ron`) | Maps Rust enums naturally, allows comments and trailing commas, readable by hand |
| References | **Stable string ids** (`s_qaLQ`, `n_IZKa`), never indices or names | Renaming and reordering never break wires, mappings, macros or bindings |
| Versioning | `format: N` and a migration for each step | Old shows must open years later |
| Missing files | The node shows black, the project still loads | A missing file must never block a show |

## 2. Layout

```
Show.velo/
├── project.ron      the document: canvas, scenes (nodes, wires, mappings, macros, variations, launch), bindings
└── shaders/         each Shader node's code: tunnel.wgsl, or name.fs for an ISF shader
```

- **Save As** makes a bundle. Saving writes only the shader files that changed, so an outside editor isn't told about the others, then `project.ron` without the code.
- A shader file name is a plain `.wgsl` or `.fs` name inside `shaders/`; anything else is refused, so a shared project can never read or write outside its folder.
- In memory each Shader node holds its code (`Node::code`); `Node::file` names its file in the bundle (ARCHITECTURE.md D18). A single-file project keeps the code inline.
- The shader files are checked twice a second: a save from an outside editor comes in as one undoable `node.set_code`. "Open in editor" in the inspector opens the file in the editor set in Preferences → Code.
- Open dialogs that can't pick a folder open a bundle through its `project.ron`; the macOS app declares `.velo` a package, so Finder shows a bundle as one file.

Outside the project, in Velo's data folder (the OS app-data folder, or `VELO_DATA` when set): preferences, the autosave and the crash lock, the AI chats and their pictures, the agents' server port and secret.

## 3. `project.ron`

Abridged from a real file:

```ron
(
    format: 4,
    name: "Untitled",
    canvas: (width: 1920, height: 1080, fps: 60),
    scenes: [(
        id: "s_qaLQ",
        name: "Tunnel",
        nodes: [(
            id: "n_IZKa",
            kind: "velo.shader",
            name: "Tunnel",
            pos: (0.0, 0.0),
            file: Some("tunnel.wgsl"),
        ), (
            id: "n_e1cN",
            kind: "velo.feedback",
            name: "Feedback",
            pos: (216.0, 0.0),
            params: { "decay": (0.85, 0.0, 0.0, 0.0), "zoom": (1.02, 0.0, 0.0, 0.0) },
            mappings: [(param: "rotate", source: "n_a1b2/6", amount: 0.15, attack_ms: 5.0, release_ms: 150.0)],
        ), (
            id: "n_vmKC",
            kind: "velo.output",
            name: "Output",
            pos: (432.0, 0.0),
        )],
        edges: [(from: ("n_IZKa", 0), to: ("n_e1cN", 0)), (from: ("n_e1cN", 0), to: ("n_vmKC", 0))],
        launch: (transition: Crossfade, duration: 4.0, quantize: Bar),
        macros: ((name: "Speed", value: 0.5, targets: [(node: "n_IZKa", param: "speed", min: 0.1, max: 2.0)]), /* … eight in all */),
    )],
    bindings: [
        (control: Midi(port: None, channel: 0, msg: Cc(21)), target: Macro(0), behavior: Absolute(takeover: Pickup)),
    ],
)
```

- `kind` is a built-in id (`velo.noise`, `velo.shader`, `velo.output`…). An unknown kind (from a newer Velo) loads and keeps its wires.
- `params` holds only the values the user changed, one vec4 each (a float uses the first number); the rest keep the shader header's defaults.
- A mapping's `source` is a signal node's output, `"<node id>/<output index>"` (here the scene's Audio In, output 6: the hat).
- `media` holds an Image or Video node's file (or folder of frames) and a Text node's font file, as an absolute path; `text` holds a Text node's words.
- UI-only data lives inline and the engine ignores it: node positions, a node's emoji `icon`, a scene's cable `ties` (each `(id, pos, cables)`, a cable named by where it ends: a wire by its input, `"n_ab12/0"`, a signal's line by the node it moves and its source, `"n_cd34 n_ef56/2"`).
- Empty lists and untouched macros are left out; maps are sorted, so saves give small diffs.

## 4. Saving safely

- Every file is written to `<name>.tmp`, synced, then renamed over the old one: a crash mid-save never corrupts a show.
- **Autosave**: every 60 s, unsaved work is written to `autosave.velo` in Velo's data folder. A `session.lock` there marks a running session; if it's still there at the next start, Velo offers the autosave.
- A file saved by a newer Velo (a higher `format`) is refused with a message instead of being opened and overwritten.

## 5. Media

Paths are absolute. Not yet: a `media/` folder in the bundle with "Collect media", relative paths, and a zipped `.velo` for sharing.

## 6. Migrations

A file parses into the current model (new fields have defaults), then `Project::migrate` brings it up step by step. Format 1 keeps a fixture file (`crates/velo-core/tests/fixtures/format-1.velo`), never regenerated; every step has a test.

| From | To | What changes |
|---|---|---|
| 1 | 2 | A mapping of a shared source (`"kick"`, `"bar_phase"`) points at the scene's Audio In or Beat node output (`"n_ab12/4"`); the node is added left of the graph when the scene has none |
| 2 | 3 | A Grid is no longer an output: where one went out, it now feeds the scene's Output node, added beside it when there's none |
| 3 | 4 | A Shader node that reads an input (one is wired in, or its header asks for a texture) becomes a Shader FX |
