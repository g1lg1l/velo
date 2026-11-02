# Velo: Scenes

What plays: scenes, their macros and variations, and how one replaces another on the output.

## 1. The idea

A scene is a look you can launch, as in Synesthesia, with launch quantization as in Ableton Live. Macros and variations come from Ravune, Velo's sibling DAW: eight knobs per scene, and snapshots of them. There is no clip grid and no separate "layer" concept: each scene is its own graph ([graph.md](graph.md)).

## 2. Model

```rust
pub struct Scene {
    pub id: String,                 // "s_7Kq2": what bindings and OSC point at
    pub name: String,
    pub icon: String,               // an emoji before its name
    pub nodes: Vec<Node>,           // the look (graph.md)
    pub edges: Vec<Edge>,
    pub ties: Vec<Tie>,
    pub launch: Launch,             // how it goes out (§3)
    pub macros: [Macro; 8],
    pub variations: Vec<Variation>, // at most 8, A to H
}
pub struct Macro { pub name: String, pub value: f32, pub targets: Vec<MacroTarget> }
pub struct MacroTarget { pub node: String, pub param: String, pub min: f32, pub max: f32 }
pub struct Variation { pub name: String, pub macros: [f32; 8] }
```

- **Macros are the controller surface that stays put.** Map a controller to Macro 1 to 8 once; each scene decides what its macros move. A target's `min` and `max` are in the parameter's units (`min` above `max` turns it down as the macro goes up).
- **One macro per parameter.** Assigning a driven parameter to another macro moves it. While driven, the parameter's own value is ignored: its slider shows where the macro has it, with an `M3` chip and the range as a band, and dragging the slider turns the macro through that range.
- **A variation is a snapshot of the eight macros.** "Build" and "Drop" can be two variations of one scene. Recall at once, or **morph** over some beats of the clock. Anything a variation should change goes under a macro, so controllers, variations and morphs always agree.
- **On the render thread** a driven parameter's base is its range at the macro's value, clamped to the shader's range; mappings add on top ([graph.md](graph.md) §5).
- **Morphing.** `variation.recall` with `morph_beats` puts the document at the variation at once (one undo step) and the engine glides the eight values there linearly on its clock. A macro turned mid-morph leaves the morph (the others go on); a rebuild mid-morph lands at once. The knobs show the destination.

Operators, all undoable: `macro.set`, `macro.rename`, `macro.assign`, `macro.unassign`, `macro.target`, `macro.randomize` (only macros that drive something; a `seed` repeats a throw), `variation.store`, `variation.recall`, `variation.remove`. Deleting a node drops its macro targets and its controller bindings. An untouched scene writes neither macros nor variations to the file.

**The deck** (under everything, the window's width, `Cmd+J`) shows the selected scene's controls while playing, whatever the inspector shows: VARIATIONS on the left (click recalls, Shift+click morphs over a bar, right-click for more, + stores, the dice throws the macros somewhere new), the eight MACROS across the middle (right-click renames), and the SOURCES on the right, to drag onto any slider.

**Templates.** The scenes rail's + offers an empty scene or a template (Velo, Tunnel, Kaleido Bloom, Pulse Rings, Liquid): small patches the music already moves, with named macros, built with the operators in one undo step. A new profile opens the demo: the Velo template, live.

## 3. Launching and transitions

```rust
pub struct Launch { pub transition: Transition, pub duration: f32, pub quantize: Quantize }
// default: Crossfade, 4 beats, Bar
```

| Setting | Options |
|---|---|
| Transition | Cut, Crossfade, Wipe (a soft edge across), Zoom (the old one rushes past as the new one settles), Glitch (16 x 9 blocks turning over, colors split), Flash (through white) |
| Duration | 1 to 32 whole beats (every transition but Cut) |
| Quantize | Off, Beat, Bar, 4 bars |

`scene.set_launch` sets them; the inspector shows them when no node is selected.

- **Which scene is live is the engine's state**, not the document's: not saved, not undoable. At start (and after opening a project) the selected scene goes live at once; after that nothing launches by itself unless the autopilot is on. Deleting the live scene leaves it on the output until another launch.
- **Boundaries are on an absolute grid** from the clock's beat 0 (a tap series' first tap or a Resync is a downbeat): Bar is the next multiple of 4 beats, 4 bars of 16. A launch exactly on a boundary waits for the next one. A transition's length turns into seconds at the tempo when it starts.
- **One launch waits at a time.** A new launch replaces the waiting one; launching the live scene calls it off. A launch landing mid-transition ends that one at once.
- **Selecting is arming.** The selected scene is always built (the preview shows it), so launching it costs nothing on the render thread. A scene that isn't built is built first, then waits for the next boundary after it's ready: a short delay, never a hitch.
- **Warm cache.** The last 4 scenes used (edited or launched) stay built, least recently used out first. The live scene, one fading out, the waiting launch and the edited one are never evicted.
- **Preview and Program.** When the selected scene isn't live, the engine renders it too, for the preview card (`PREVIEW · name`, with Launch and the countdown) and the Output node's viewer, while the outputs keep the live one.
- **A transition is the output's blit:** one pass that samples both pictures and mixes them straight into each output and the preview. Both scenes render while it runs.

**Autopilot.** The airplane in the scenes rail launches the next scene (in order, or at random but never the one going out) every 4, 8, 16 or 32 bars, or every so many seconds, a beat early so its own boundary lands on the turn; and, if asked, on a drop (the music back at full after a quieter while, at most once every 4 bars). It is off at every start; its settings are kept.

## 4. Controls that touch scenes

| Gesture | Bindings |
|---|---|
| Launch scene n | Keys `1` to `9`; right-click a card, Launch; a MIDI note, CC or Program Change |
| Launch the selected scene | A tap of `Space` (a hold with a drag pans the canvas); the preview card's Launch; MIDI |
| Recall a variation | Click it on the deck; MIDI |
| Morph to a variation | Shift+click it (one bar) |

Controllers drive the scene on the output, not the selected one: macros and variations from MIDI go to the live scene. After a launch a knob that no longer matches its macro is in **pickup** until it crosses the value (or jumps, per binding); see [control.md](control.md) §2.

## 5. Not yet

A setlist or cue list; durations in seconds and 8-bar quantize; transitions of your own (shaders with two inputs and a `progress`); a manual T-bar; launch thumbnails; scene import and export between projects.
