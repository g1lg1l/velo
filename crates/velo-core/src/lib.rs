//! Velo core: the project model and the operator system.
//! From Ravune (bd10814), trimmed for Velo.
//!
//! Pure data: no GPU, no audio, no UI. Every edit goes through an [`Operator`], so the
//! UI, keymaps, the command palette and (later) MIDI, scripts and agents share one undoable API.

mod ops;
#[cfg(test)]
mod tests;

pub use ops::{OPERATORS, Operator, find};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Linear,
    /// Knob travel is logarithmic (sizes, frequencies, times).
    Log,
    /// Integer steps.
    Steps,
}

/// A parameter's range and how a knob shows it.
#[derive(Debug)]
pub struct ParamInfo {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub curve: Curve,
    pub unit: &'static str,
}

impl ParamInfo {
    pub fn clamp(&self, v: f32) -> f32 {
        let v = v.clamp(self.min, self.max);
        if self.curve == Curve::Steps { v.round() } else { v }
    }

    /// Value → 0..1 knob position.
    pub fn to_norm(&self, v: f32) -> f32 {
        match self.curve {
            Curve::Log => (v / self.min).ln() / (self.max / self.min).ln(),
            _ => (v - self.min) / (self.max - self.min),
        }
        .clamp(0.0, 1.0)
    }

    /// 0..1 knob position → value.
    pub fn from_norm(&self, n: f32) -> f32 {
        let n = n.clamp(0.0, 1.0);
        self.clamp(match self.curve {
            Curve::Log => self.min * (self.max / self.min).powf(n),
            _ => self.min + n * (self.max - self.min),
        })
    }

    pub fn format(&self, v: f32) -> String {
        match self.unit {
            "onoff" => if v >= 0.5 { "On" } else { "Off" }.into(),
            // an int with named values: "|Normal|Add|Screen"
            unit if unit.starts_with('|') => unit[1..].split('|').nth(v.round().max(0.0) as usize).unwrap_or("?").into(),
            // an LFO's length: whole beats, or a fraction of one
            "beats" if v < 1.0 => format!("1/{:.0} beat", 1.0 / v.max(0.01)),
            "beats" => format!("{v:.1} beats"),
            // a count (segments, sides): whole numbers
            "" if self.curve == Curve::Steps => format!("{v:.0}"),
            // a plain amount (speed, zoom): two decimals
            "" => format!("{v:.2}"),
            "%" => format!("{:.0} %", v * 100.0),
            // an angle: its range is a whole turn, so 360° across it
            "°" => format!("{:.0}°", v * 360.0 / (self.max - self.min)),
            unit => format!("{v:.0} {unit}"),
        }
    }
}

const fn canvas_param(name: &'static str, (min, max, default): (f32, f32, f32), curve: Curve, unit: &'static str) -> ParamInfo {
    ParamInfo { name, min, max, default, curve, unit }
}

/// The most pixels a canvas takes (4096 × 2304, a little over 4K): every node of every scene
/// held draws a picture this big, in half floats.
pub const CANVAS_PIXELS: u64 = 4096 * 2304;

/// `width` × `height` brought down to [`CANVAS_PIXELS`] if it's more, its shape kept.
pub fn fit_canvas(width: u32, height: u32) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= CANVAS_PIXELS {
        return (width, height);
    }
    let k = (CANVAS_PIXELS as f64 / pixels as f64).sqrt();
    (((f64::from(width) * k) as u32).max(16), ((f64::from(height) * k) as u32).max(16))
}

/// The canvas' width, height and frame rate, in that order (`project.set_canvas` clamps to them).
pub const CANVAS: [ParamInfo; 3] = [canvas_param("Width", (16.0, 8192.0, 1920.0), Curve::Log, "px"), canvas_param("Height", (16.0, 8192.0, 1080.0), Curve::Log, "px"), canvas_param("Frame rate", (1.0, 240.0, 60.0), Curve::Steps, "fps")];

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// The project file format: bumped, with a migration, whenever saved files change shape.
pub const FORMAT: u32 = 4;

/// Everything that is saved and undoable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub format: u32,
    pub name: String,
    pub canvas: Canvas,
    pub scenes: Vec<Scene>,
    /// What controllers do (docs/control.md §2): the same for every scene.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
}

impl Default for Project {
    fn default() -> Self {
        Self { format: FORMAT, name: "Untitled".into(), canvas: Canvas::default(), scenes: Vec::new(), bindings: Vec::new() }
    }
}

impl Project {
    /// A file of an older format brought to this one. 1 → 2: a mapping of a shared source
    /// ("kick") comes from the scene's Audio In or Beat node, added where there's none. 2 → 3:
    /// a Grid isn't an output any more, so where one went out it now feeds the Output node.
    pub fn migrate(&mut self) {
        // a hand-edited canvas inside what the GPU can draw
        let c = &mut self.canvas;
        for (field, info) in [(&mut c.width, &CANVAS[0]), (&mut c.height, &CANVAS[1]), (&mut c.fps, &CANVAS[2])] {
            *field = info.clamp(*field as f32).round() as u32;
        }
        (c.width, c.height) = fit_canvas(c.width, c.height);
        if self.format < 2 {
            for c in 0..self.scenes.len() {
                for n in 0..self.scenes[c].nodes.len() {
                    for m in 0..self.scenes[c].nodes[n].mappings.len() {
                        let source = self.scenes[c].nodes[n].mappings[m].source.clone();
                        if let Some(key) = ops::signal_source(self, c, &source) {
                            self.scenes[c].nodes[n].mappings[m].source = key;
                        }
                    }
                }
            }
        }
        if self.format < 3 {
            for scene in &mut self.scenes {
                ops::grid_out(scene);
            }
        }
        if self.format < 4 {
            for scene in &mut self.scenes {
                ops::shader_fx(scene);
            }
        }
        self.format = self.format.max(FORMAT);
    }

    pub fn scene(&self, id: &str) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.id == id)
    }

    /// The binding that moves or fires `target`, if any.
    pub fn binding(&self, target: &Target) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.target == *target)
    }
}

/// What every output renders: its size in pixels and frames per second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Default for Canvas {
    fn default() -> Self {
        Self { width: 1920, height: 1080, fps: 60 }
    }
}

/// One look you can launch: one graph of nodes (docs/graph.md), its eight macros and their
/// variations (docs/scenes.md §2).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// Stable id ("s_7Kq2"): what mappings, bindings and OSC point at, so renaming and
    /// reordering never break them.
    pub id: String,
    pub name: String,
    /// Files from before the node canvas have neither: they load as empty graphs.
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    /// How it goes out when launched (files from before launching load with the defaults).
    #[serde(default)]
    pub launch: Launch,
    /// What a controller's eight knobs drive in this scene (files from before macros load
    /// with eight unnamed ones at 0, driving nothing).
    #[serde(default, skip_serializing_if = "untouched")]
    pub macros: [Macro; MACROS],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variations: Vec<Variation>,
    /// An emoji shown before its name (none: its number alone).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    /// Cables tied into bundles on its canvas.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ties: Vec<Tie>,
    /// A layer shows over or under whichever scene goes out, combined with it as `blend` says;
    /// launching one switches it on or off.
    #[serde(default, skip_serializing_if = "is_default")]
    pub layer: Layer,
    #[serde(default, skip_serializing_if = "is_default")]
    pub blend: LayerBlend,
    /// Left out of the autopilot's turns (a layer always is).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// What a scene is to the show (docs/scenes.md §4): one launched like the others, or a layer
/// always over or under whichever of them goes out (a title, a logo, a background).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layer {
    #[default]
    Scene,
    Top,
    Bottom,
}

/// How a layer and what's under it go together: the upper one over the lower through its
/// transparency, or added, screened (lights on black) or multiplied (darks on white).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerBlend {
    Over,
    Add,
    #[default]
    Screen,
    Multiply,
}

impl LayerBlend {
    pub const ALL: [LayerBlend; 4] = [LayerBlend::Over, LayerBlend::Add, LayerBlend::Screen, LayerBlend::Multiply];
}

// ---------------------------------------------------------------------------
// Macros and variations (docs/scenes.md §2): the controller surface that stays put across
// scenes; each scene decides what its macros move. From Ravune (03cb268), adapted for Velo.
// ---------------------------------------------------------------------------

/// Macros per scene: what a controller's eight knobs drive.
pub const MACROS: usize = 8;
/// Variations a scene keeps at most (A to H).
pub const VARIATIONS: usize = 8;

/// One of a scene's macro knobs: at `value` (0..1) each target sits that far from its `min`
/// to its `max`. The engine applies it as the parameter's base; mappings add on top.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Macro {
    /// Empty: the interface calls it "Macro n".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub value: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<MacroTarget>,
}

/// A parameter a macro moves, between two values in the parameter's own units (`min` above
/// `max` turns it down as the macro goes up).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MacroTarget {
    pub node: String,
    /// The shader parameter's id; a vec2's axis as "offset.x" (as mappings name them).
    pub param: String,
    pub min: f32,
    pub max: f32,
}

impl MacroTarget {
    /// The parameter's value with the macro at `v` (0..1).
    pub fn at(&self, v: f32) -> f32 {
        self.min + (self.max - self.min) * v.clamp(0.0, 1.0)
    }
}

fn untouched(macros: &[Macro; MACROS]) -> bool {
    macros.iter().all(|m| *m == Macro::default())
}

/// A snapshot of the eight macros, to recall at once or morph to over some beats.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Variation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub macros: [f32; MACROS],
}

impl Scene {
    /// The macro driving a parameter (by its mapping name, "zoom" or "offset.x"), and how.
    pub fn driver(&self, node: &str, param: &str) -> Option<(usize, &MacroTarget)> {
        self.macros.iter().enumerate().find_map(|(i, m)| m.targets.iter().find(|t| t.node == node && t.param == param).map(|t| (i, t)))
    }
}

// ---------------------------------------------------------------------------
// Bindings (docs/control.md §2): a controller's knob or pad doing something. Project data, the
// same in every scene.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub control: Control,
    pub target: Target,
    pub behavior: Behavior,
}

/// Which physical control: for now a MIDI message on a port (by name; `None` is any port, so a
/// project moves between machines) and a channel (0..15).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Control {
    Midi { port: Option<String>, channel: u8, msg: MidiMsg },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MidiMsg {
    /// A controller (knob, fader, a pad sending CCs).
    Cc(u8),
    /// A key or pad: note-on fires, the velocity is the value.
    Note(u8),
    /// Any program change on the channel: program n launches scene n (counted from a
    /// `LaunchScene` target's index, so `LaunchScene(0)` makes program 0 the first scene).
    ProgramChange,
    /// A 14-bit controller: CC n (0..31) the coarse half, CC n + 32 the fine one.
    Cc14(u8),
}

/// What a control does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// Macro n (0..7) of the scene on the output.
    Macro(u8),
    /// A node's parameter (node ids are unique in the project), by its mapping name.
    Param {
        node: String,
        param: String,
    },
    /// The scene at this index (0 is the first).
    LaunchScene(usize),
    /// The scene selected in the editor.
    LaunchSelected,
    /// Variation n (0 is A) of the scene on the output.
    Variation(usize),
    Blackout,
    Freeze,
    TapTempo,
}

/// How a control's values become actions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Behavior {
    /// A knob or fader sets the value along its travel.
    Absolute { takeover: Takeover },
    /// An endless encoder: each message turns the value by `step` of its travel a click, the
    /// clicks encoded as `mode` says.
    Relative { mode: RelMode, step: f32 },
    /// A press (note-on, a CC going past the middle) fires once.
    Trigger,
    /// Each press flips it (a value between its ends, blackout on and off).
    Toggle,
}

/// How an endless encoder says how far it turned (the three dialects controllers use).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelMode {
    /// 1, 2… clockwise; 127, 126… counter-clockwise.
    TwosComplement,
    /// 65, 66… clockwise; 63, 62… counter-clockwise.
    BinaryOffset,
    /// 1, 2… clockwise; 65, 66… counter-clockwise.
    SignMagnitude,
}

impl RelMode {
    pub const ALL: [RelMode; 3] = [RelMode::TwosComplement, RelMode::BinaryOffset, RelMode::SignMagnitude];

    /// The clicks a value means: positive clockwise.
    pub fn clicks(self, value: u8) -> i32 {
        let v = i32::from(value & 127);
        match self {
            RelMode::TwosComplement => {
                if v < 64 {
                    v
                } else {
                    v - 128
                }
            }
            RelMode::BinaryOffset => v - 64,
            RelMode::SignMagnitude => {
                if v < 64 {
                    v
                } else {
                    -(v - 64)
                }
            }
        }
    }

    /// The dialect a few messages from one encoder speak, if they can only be an encoder's:
    /// small steps around 0, or around 64, never a knob's travel.
    pub fn guess(values: &[u8]) -> Option<RelMode> {
        let near = |v: u8, c: i32| (i32::from(v) - c).abs() <= 4 && i32::from(v) != c;
        if values.len() < 3 {
            return None;
        }
        if values.iter().all(|&v| near(v, 0) || near(v, 128)) {
            Some(RelMode::TwosComplement)
        } else if values.iter().all(|&v| near(v, 64)) {
            Some(RelMode::BinaryOffset)
        } else {
            None
        }
    }
}

/// What a knob does when it doesn't match the value it moves (after a scene launch changed
/// the macros): `Jump` sets it at once, `Pickup` waits until the knob crosses the value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Takeover {
    Jump,
    Pickup,
}

// ---------------------------------------------------------------------------
// Launching (docs/scenes.md §3). Which scene is live is the engine's state, not the document.
// ---------------------------------------------------------------------------

/// A scene's launch settings: how it replaces the live scene, and on which boundary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Launch {
    pub transition: Transition,
    /// The crossfade's length, in beats.
    pub duration: f32,
    pub quantize: Quantize,
}

impl Default for Launch {
    fn default() -> Self {
        Self { transition: Transition::Crossfade, duration: LAUNCH_BEATS.default, quantize: Quantize::Bar }
    }
}

/// A crossfade's length: whole beats, 1 to 32.
pub const LAUNCH_BEATS: ParamInfo = canvas_param("Beats", (1.0, 32.0, 4.0), Curve::Steps, "");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transition {
    /// The new scene replaces the old one at once.
    Cut,
    /// Both render, blended over the launch's duration.
    Crossfade,
    /// The new one sweeps in from the left, a soft edge ahead of it.
    Wipe,
    /// The old one rushes past (zooming in, fading) as the new one settles from further away.
    Zoom,
    /// Blocks of the new one break in at random, colors split, until it's whole.
    Glitch,
    /// Through white: the old one burns out, the new one comes up out of the light.
    Flash,
}

impl Transition {
    /// Every one, in the order the inspector lists them.
    pub const ALL: [Transition; 6] = [Transition::Cut, Transition::Crossfade, Transition::Wipe, Transition::Zoom, Transition::Glitch, Transition::Flash];

    /// Its name, as the inspector shows it.
    pub fn label(self) -> &'static str {
        match self {
            Transition::Cut => "Cut",
            Transition::Crossfade => "Crossfade",
            Transition::Wipe => "Wipe",
            Transition::Zoom => "Zoom",
            Transition::Glitch => "Glitch",
            Transition::Flash => "Flash",
        }
    }
}

/// The boundary a launch waits for on the beat clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quantize {
    /// The next frame.
    Off,
    Beat,
    Bar,
    Bars4,
}

impl Quantize {
    pub const ALL: [Quantize; 4] = [Quantize::Off, Quantize::Beat, Quantize::Bar, Quantize::Bars4];

    /// The grid it snaps to, in beats (4/4): 0 for none.
    pub fn beats(self) -> f64 {
        match self {
            Quantize::Off => 0.0,
            Quantize::Beat => 1.0,
            Quantize::Bar => 4.0,
            Quantize::Bars4 => 16.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Graph: nodes and wires (docs/graph.md). Texture ports only for now.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// Makes an image from nothing.
    Generator,
    /// Changes one image.
    Effect,
    /// Combines images.
    Composite,
    /// Where a scene's image leaves the graph.
    /// Routes pictures: which shows (Switch), where (Grid), in what order (Layer Stack).
    Control,
    Output,
    /// Makes a number, not an image: a source for mappings (an LFO).
    Signal,
}

impl Category {
    pub const ALL: [Category; 6] = [Category::Generator, Category::Effect, Category::Composite, Category::Control, Category::Output, Category::Signal];

    pub fn label(self) -> &'static str {
        match self {
            Category::Generator => "Generators",
            Category::Effect => "Effects",
            Category::Composite => "Composite",
            Category::Control => "Control",
            Category::Output => "Output",
            Category::Signal => "Signals",
        }
    }
}

/// What a node is: its ports, by name. The catalog is static data so the UI, files and tests
/// can reason about graphs without a GPU.
#[derive(Debug)]
pub struct NodeKind {
    /// Stable id saved in files ("velo.noise"; user shaders will be "user.…").
    pub id: &'static str,
    pub label: &'static str,
    pub category: Category,
    pub inputs: &'static [&'static str],
    pub outputs: &'static [&'static str],
}

impl NodeKind {
    /// Its group in the node menu: its category's, except that the signal nodes working on
    /// other signals (remap, count, hold, compare…) sit apart, as Logic, from the ones that
    /// make them (audio, the beat, LFOs, time, random values, a picture read).
    pub fn group(&self) -> &'static str {
        if self.category == Category::Signal && ![AUDIO, CLOCK, LFO, TIME, RANDOM, PROBE].contains(&self.id) { "Logic" } else { self.category.label() }
    }
}

const fn kind(id: &'static str, label: &'static str, category: Category, inputs: &'static [&'static str], outputs: &'static [&'static str]) -> NodeKind {
    NodeKind { id, label, category, inputs, outputs }
}

const IN: &[&str] = &["in"];
const OUT: &[&str] = &["out"];

/// Every built-in node, in the order the node menu lists them.
pub const NODE_KINDS: &[NodeKind] = &[
    kind("velo.noise", "Noise", Category::Generator, &[], OUT),
    kind("velo.gradient", "Gradient", Category::Generator, &[], OUT),
    kind("velo.shapes", "Shapes", Category::Generator, &[], OUT),
    kind("velo.waveform", "Waveform", Category::Generator, &[], OUT),
    kind(SHADER, "Shader", Category::Generator, &[], OUT),
    kind(IMAGE, "Image", Category::Generator, &[], OUT),
    kind(VIDEO, "Video", Category::Generator, &[], OUT),
    kind(MEDIA_LIST, "Media List", Category::Generator, &[], OUT),
    kind(TEXT, "Text", Category::Generator, &[], OUT),
    kind(TEXT_LIST, "Text List", Category::Generator, &[], OUT),
    kind(PARTICLES, "Particles", Category::Generator, &[], OUT),
    kind("velo.transform", "Transform", Category::Effect, IN, OUT),
    kind("velo.kaleido", "Kaleido", Category::Effect, IN, OUT),
    kind("velo.color", "Color", Category::Effect, IN, OUT),
    kind("velo.blur", "Blur", Category::Effect, IN, OUT),
    kind(FEEDBACK, "Feedback", Category::Effect, IN, OUT),
    kind("velo.displace", "Displace", Category::Effect, &["in", "map"], OUT),
    kind("velo.opacity", "Opacity", Category::Effect, IN, OUT),
    kind("velo.levels", "Levels", Category::Effect, IN, OUT),
    kind("velo.threshold", "Threshold", Category::Effect, IN, OUT),
    kind("velo.edges", "Edges", Category::Effect, IN, OUT),
    kind("velo.channels", "Channels", Category::Effect, IN, OUT),
    kind("velo.crop", "Crop", Category::Effect, IN, OUT),
    kind("velo.polar", "Polar", Category::Effect, IN, OUT),
    kind("velo.uvmap", "UV Map", Category::Effect, &["in", "map"], OUT),
    kind("velo.lookup", "Lookup", Category::Effect, &["in", "palette"], OUT),
    kind("velo.freeze", "Freeze", Category::Effect, IN, OUT),
    kind(SHADER_FX, "Shader FX", Category::Effect, &["src"], OUT),
    kind(BLEND, "Blend", Category::Composite, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p"], OUT),
    kind("velo.mask", "Mask", Category::Composite, &["in", "mask"], OUT),
    kind("velo.merge", "Merge", Category::Composite, &["r", "g", "b", "a"], OUT),
    kind(SWITCH, "Switch", Category::Control, &["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16"], OUT),
    kind(GRID, "Grid", Category::Control, &["1", "2", "3", "4", "5", "6", "7", "8"], OUT),
    kind("velo.layers", "Layer Stack", Category::Control, &["1", "2", "3", "4", "5", "6", "7", "8"], OUT),
    kind("velo.output", "Output", Category::Output, IN, &[]),
    kind(AUDIO, "Audio In", Category::Signal, &[], &["level", "bass", "mid", "high", "kick", "snare", "hat", "melody"]),
    kind(CLOCK, "Beat", Category::Signal, &[], &["beat", "bar"]),
    kind(LFO, "LFO", Category::Signal, &[], OUT),
    kind(TIME, "Time", Category::Signal, &[], OUT),
    kind(RANDOM, "Random", Category::Signal, &["trigger"], OUT),
    kind(PROBE, "Probe", Category::Signal, IN, OUT),
    kind(REMAP, "Remap", Category::Signal, IN, OUT),
    kind(MATH, "Math", Category::Signal, &["a", "b"], OUT),
    kind(ENVELOPE, "Envelope", Category::Signal, IN, OUT),
    kind(COUNTER, "Counter", Category::Signal, &["in", "reset"], OUT),
    kind(SEQUENCER, "Sequencer", Category::Signal, &[], OUT),
    kind(HOLD, "Sample & Hold", Category::Signal, &["in", "trigger"], OUT),
    kind(TOGGLE, "Toggle", Category::Signal, IN, OUT),
    kind(COMPARE, "Compare", Category::Signal, &["a", "b"], OUT),
    kind(TIMER, "Timer", Category::Signal, &["reset"], OUT),
    kind(SMOOTH, "Smooth", Category::Signal, IN, OUT),
    kind(QUANTIZE, "Quantize", Category::Signal, IN, OUT),
    kind(SPEED, "Speed", Category::Signal, &["in", "reset"], OUT),
    kind(DELAY, "Delay", Category::Signal, IN, OUT),
];

/// Up to sixteen pictures merged in one, as many inputs shown as its `inputs` parameter says.
pub const BLEND: &str = "velo.blend";

/// How many inputs a node's card shows: a Blend as many as its `inputs` parameter asks, a
/// Switch its `count` (2 to 16), never fewer than are wired; any other kind all it has.
pub fn inputs_shown(scene: &Scene, n: &Node) -> usize {
    let Some(k) = node_kind(&n.kind) else { return 0 };
    let param = match n.kind.as_str() {
        BLEND => "inputs",
        SWITCH => "count",
        _ => return k.inputs.len(),
    };
    let asked = n.params.get(param).map_or(2, |v| v[0].round().max(0.0) as usize);
    let wired = scene.edges.iter().filter(|e| e.to.0 == n.id).map(|e| usize::from(e.to.1) + 1).max().unwrap_or(0);
    asked.max(wired).clamp(2, k.inputs.len())
}

/// A parameter as the engine runs it: as set, but a Blend's `inputs` counts every wired one
/// too (its shader reads only that many).
pub fn engine_param(scene: &Scene, n: &Node, id: &str) -> Option<[f32; 4]> {
    if n.kind == BLEND && id == "inputs" {
        return Some([inputs_shown(scene, n) as f32, 0.0, 0.0, 0.0]);
    }
    n.params.get(id).copied()
}

/// Reads its input as it was last frame: the one node that may close a loop.
pub const FEEDBACK: &str = "velo.feedback";
/// A picture from a file ([`Node::media`]): a still, an animated GIF, or a folder of frames.
pub const IMAGE: &str = "velo.image";
/// A movie from a file ([`Node::media`]): HAP, or (macOS) H.264, HEVC, ProRes.
pub const VIDEO: &str = "velo.video";

/// Up to 4 × 4 cells of eight inputs, one picture out (it was an output node until format 3).
pub const GRID: &str = "velo.grid";
/// Where a scene's picture leaves the graph.
pub const OUTPUT: &str = "velo.output";
/// One of up to eight inputs, as its `value` picks, with a cut or a transition between them.
pub const SWITCH: &str = "velo.switch";

/// Words ([`Node::text`]) set in a font, colored, sized and placed by its parameters.
pub const TEXT: &str = "velo.text";
/// A Text whose lines ([`Node::text`]) show one at a time, on the beat or the seconds.
pub const TEXT_LIST: &str = "velo.textlist";

/// The nodes that set words: a Text or a Text List.
pub fn sets_text(kind: &str) -> bool {
    kind == TEXT || kind == TEXT_LIST
}

/// The words a new node of this kind starts with.
pub fn first_text(kind: &str) -> Option<String> {
    match kind {
        TEXT => Some("Text".into()),
        TEXT_LIST => Some(join_items(&["One", "Two", "Three"])),
        _ => None,
    }
}

/// What starts each of a Text List's items in its words ([`Node::text`]), so an item can run
/// over several lines: the record separator.
pub const ITEM: char = '\u{1E}';

/// A Text List's items: its words split where each one starts ([`ITEM`]). Words without one
/// (a list from before items could run over lines, a Text's) are an item a line.
pub fn text_items(text: &str) -> Vec<&str> {
    if text.contains(ITEM) { text.split(ITEM).skip(usize::from(text.starts_with(ITEM))).collect() } else { text.lines().collect() }
}

/// A Text List's items as its words.
pub fn join_items<S: AsRef<str>>(items: &[S]) -> String {
    items.iter().map(|i| format!("{ITEM}{}", i.as_ref())).collect()
}

/// Up to [`MEDIA_LIST_FILES`] pictures and movies played in turn ([`Node::media`], a path a line).
pub const MEDIA_LIST: &str = "velo.medialist";
/// How many files a Media List holds.
pub const MEDIA_LIST_FILES: usize = 8;

/// A Media List's files, in the order it plays them.
pub fn media_list(n: &Node) -> Vec<&str> {
    n.media.as_deref().map_or_else(Vec::new, |m| m.lines().map(str::trim).filter(|l| !l.is_empty()).take(MEDIA_LIST_FILES).collect())
}

/// Sprites thrown from an emitter: soft dots, or the picture of its file ([`Node::media`]).
pub const PARTICLES: &str = "velo.particles";

/// The nodes that show a file: an Image or a Video node.
pub fn shows_media(kind: &str) -> bool {
    kind == IMAGE || kind == VIDEO
}
/// The node that runs its own WGSL ([`Node::code`]): a picture.
pub const SHADER: &str = "velo.shader";
/// Its own WGSL on what's wired in (until format 4, a Shader node with an input).
pub const SHADER_FX: &str = "velo.shader_fx";

/// A node that runs its own code: a Shader or a Shader FX.
pub fn is_shader(kind: &str) -> bool {
    kind == SHADER || kind == SHADER_FX
}
/// A low-frequency oscillator on the beat clock: a signal node, a mapping's source by its id.
pub const LFO: &str = "velo.lfo";
/// What the audio input hears, as a signal node: one output per feature.
pub const AUDIO: &str = "velo.audio";
/// The beat clock as a signal node: a hit on every beat, the ramp through the bar.
pub const CLOCK: &str = "velo.clock";
/// Time going on: 0 to 1 over a number of seconds, again and again (colors that drift).
pub const TIME: &str = "velo.time";
/// A signal brought into the range you want: auto range, curve, smoothing, gate, invert.
pub const REMAP: &str = "velo.remap";
/// Two signals into one: add, subtract, multiply, min, max, average, difference.
pub const MATH: &str = "velo.math";
/// A shape played on each hit of its input: attack, release, curve.
pub const ENVELOPE: &str = "velo.envelope";
/// A step on each hit of its input, through a number of values; a reset input.
pub const COUNTER: &str = "velo.counter";
/// A value a step, 2 to 16 steps, on the beat clock: a pattern of its own.
pub const SEQUENCER: &str = "velo.sequencer";
/// On each hit of its trigger, its input's value (or a random one) until the next.
pub const HOLD: &str = "velo.hold";
/// 0 or 1, the other one on each hit of its input.
pub const TOGGLE: &str = "velo.toggle";
/// 1 while a is above (or below) b or a fixed value, else 0.
pub const COMPARE: &str = "velo.compare";
/// A hit each time its seconds have gone by, again and again; a hit on `reset` starts it over.
pub const TIMER: &str = "velo.timer";
/// A new random value every so many beats, or on each hit of its input; gliding to it or not.
pub const RANDOM: &str = "velo.random";
/// Its input followed at a steady speed (glide) or slowing as it nears (lag), up and down apart.
pub const SMOOTH: &str = "velo.smooth";
/// Its input snapped to a number of even steps.
pub const QUANTIZE: &str = "velo.quantize";
/// A cycle its input drives: how fast it goes, not where it is, so motion never jumps.
pub const SPEED: &str = "velo.speed";
/// Its input as it was a number of beats ago.
pub const DELAY: &str = "velo.delay";
/// A picture read as a number: its brightness (or a channel) over a region, or at a point. The
/// one signal node a picture goes into.
pub const PROBE: &str = "velo.probe";

/// A mapping's source when it's a signal node's output: "n_ab12/4" (or "n_ab12", its first),
/// as the node's id and the output's index.
pub fn signal_output(source: &str) -> Option<(&str, u8)> {
    if !source.starts_with("n_") {
        return None;
    }
    match source.split_once('/') {
        Some((id, port)) => Some((id, port.parse().ok()?)),
        None => Some((source, 0)),
    }
}
/// A shared source ("kick", "bar_phase") as a signal node's output: an Audio In's, a Beat's.
pub fn shared_signal(source: &str) -> Option<(&'static str, u8)> {
    let i = SOURCES.iter().position(|s| *s == source)?;
    Some(match i {
        0..7 => (AUDIO, i as u8),
        7 | 8 => (CLOCK, (i - 7) as u8),
        // the melody came later: the Audio In's eighth output
        _ => (AUDIO, 7),
    })
}
/// The LFO's shapes, in its `shape` parameter's order.
pub const LFO_SHAPES: [&str; 5] = ["Sine", "Triangle", "Saw", "Square", "Random"];

pub fn node_kind(id: &str) -> Option<&'static NodeKind> {
    NODE_KINDS.iter().find(|k| k.id == id)
}

/// A node on the canvas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Stable id ("n_ab12"), unique in the project: what wires point at.
    pub id: String,
    /// A [`NodeKind`] id. Unknown kinds (a newer file) load and keep their wires.
    pub kind: String,
    pub name: String,
    /// Turned off: an effect passes its first input through, anything else gives black, a
    /// signal node gives nothing (docs/graph.md).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
    /// Top-left corner on the canvas, in canvas units.
    pub pos: [f32; 2],
    /// Parameter values the user changed, by the shader's parameter id; the rest keep the
    /// shader header's defaults. One vec4 each, as the GPU takes them (a float uses `x`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, [f32; 4]>,
    /// Sources driving its parameters (docs/graph.md §3), in the order they were added.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mappings: Vec<Mapping>,
    /// A Shader node's WGSL, header and all (docs/plugins.md §2). A project bundle keeps it in
    /// `shaders/` as [`Self::file`] (project-format.md §2); a single-file project, inline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// A Shader node's file in the bundle's `shaders/` ("tunnel.wgsl"): named after the node
    /// when it's added, unique in the project, kept when the node is renamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// An Image node's picture: an image file, an animated GIF, or a folder of numbered frames
    /// (an image sequence); a Video node's movie; a Text node's own font file. An absolute path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<String>,
    /// A Text node's words; a newline starts another line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// An emoji shown before its name (none: its category's dot).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
}

/// A file name for a new Shader node called `name`, not used by any node in `project`:
/// "Tunnel 2" → "tunnel-2.wgsl", "tunnel-2-2.wgsl" if that's taken; `ext` "fs" for an ISF one.
pub fn shader_file(project: &Project, name: &str, ext: &str) -> String {
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    let slug = slug.split('-').filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    let base = if slug.is_empty() { "shader".to_string() } else { slug };
    let taken = |f: &str| project.scenes.iter().flat_map(|s| &s.nodes).any(|n| n.file.as_deref() == Some(f));
    std::iter::once(format!("{base}.{ext}")).chain((2..).map(|i| format!("{base}-{i}.{ext}"))).find(|f| !taken(f)).expect("a free name")
}

/// What a mapping can listen to, in the order the engine samples them each frame: the audio
/// features (0..1), then the beat clock (a hit on every beat, the ramp through the bar).
pub const SOURCES: [&str; 10] = ["level", "bass", "mid", "high", "kick", "snare", "hat", "beat", "bar_phase", "melody"];

/// A source moving a parameter: each frame the engine smooths the source (one-pole, its own
/// attack and release, so one Kick can punch the zoom and nudge the hue softly) and adds
/// `amount` × the parameter's range × that level to the base value, clamped to the range.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    /// The shader parameter's id ("zoom"); a vec2's axis as "offset.x" / "offset.y".
    pub param: String,
    /// One of [`SOURCES`], or a signal node's output ([`signal_output`]).
    pub source: String,
    /// -1..1 of the parameter's range: 0.5 moves it half its travel at full level.
    pub amount: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    /// The source is read upside down: 1 in silence, 0 on a hit.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub invert: bool,
}

impl Mapping {
    pub const AMOUNT: f32 = 0.5;
    pub const ATTACK_MS: f32 = 5.0;
    pub const RELEASE_MS: f32 = 150.0;
    /// The longest smoothing a mapping takes, in ms.
    pub const MAX_MS: f32 = 5000.0;
}

/// A wire from an output to an input, each as (node id, port index). An input takes one wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: (String, u8),
    pub to: (String, u8),
}

/// A tie gathering cables into a bundle where it sits on the canvas (canvas units). Each cable
/// by where it ends: a wire by its input ("n_ab12/0"), a signal's line by the node it moves and
/// its source ("n_cd34 n_ef56/2"). A cable that's gone is left out of the bundle.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tie {
    pub id: String,
    pub pos: [f32; 2],
    pub cables: Vec<String>,
}

impl Tie {
    /// How a tie names a wire: by its input.
    pub fn wire(to: &(String, u8)) -> String {
        format!("{}/{}", to.0, to.1)
    }

    /// How a tie names a signal's line: by the node it moves and its source.
    pub fn line(node: &str, source: &str) -> String {
        format!("{node} {source}")
    }
}

impl Scene {
    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// A shared source ("kick") as this scene's signal node output ("n_ab12/4"), when the
    /// scene has the node (its first Audio In or Beat).
    pub fn signal_key(&self, source: &str) -> Option<String> {
        let (kind, port) = shared_signal(source)?;
        self.nodes.iter().find(|n| n.kind == kind).map(|n| format!("{}/{port}", n.id))
    }

    /// The wire into an input, if any.
    pub fn wire_into(&self, node: &str, port: u8) -> Option<&Edge> {
        self.edges.iter().find(|e| e.to.0 == node && e.to.1 == port)
    }

    /// Whether an output may be wired into an input (replacing the input's wire, if it has one):
    /// both ports exist, no node feeds itself, and no loop closes except through a Feedback
    /// node, which reads the previous frame (so evaluation stays a topological sort).
    pub fn can_connect(&self, from: (&str, u8), to: (&str, u8)) -> Result<(), String> {
        let ports = |id: &str, output: bool| {
            let node = self.node(id).ok_or_else(|| format!("no node {id}"))?;
            let kind = node_kind(&node.kind).ok_or_else(|| format!("unknown node kind {}", node.kind))?;
            Ok::<_, String>((node, if output { kind.outputs.len() } else { kind.inputs.len() }))
        };
        let (_, outs) = ports(from.0, true)?;
        let (target, ins) = ports(to.0, false)?;
        if usize::from(from.1) >= outs || usize::from(to.1) >= ins {
            return Err("no such port".into());
        }
        if from.0 == to.0 {
            return Err("A node can't feed itself.".into());
        }
        // numbers go into numbers: a signal into a signal node (a parameter takes it as a mapping),
        // a picture into a picture node or a Probe, which reads it
        let signal = |id: &str| self.node(id).and_then(|n| node_kind(&n.kind)).is_some_and(|k| k.category == Category::Signal);
        if signal(from.0) != (signal(to.0) && target.kind != PROBE) {
            return Err(if signal(from.0) { "A signal goes into a signal node, or onto a node's card to move one of its parameters." } else { "A picture goes into a signal node through a Probe." }.into());
        }
        if target.kind == FEEDBACK {
            return Ok(());
        }
        // a loop: the source is downstream of the target, not counting wires into Feedback
        let mut stack = vec![to.0];
        let mut seen = vec![to.0];
        while let Some(at) = stack.pop() {
            for e in self.edges.iter().filter(|e| e.from.0 == at) {
                let next = e.to.0.as_str();
                if self.node(next).is_some_and(|n| n.kind == FEEDBACK) || seen.contains(&next) {
                    continue;
                }
                if next == from.0 {
                    return Err("That makes a loop: only a Feedback node can feed back.".into());
                }
                seen.push(next);
                stack.push(next);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Session: project + undo
// ---------------------------------------------------------------------------

/// The project plus its undo history.
#[derive(Default)]
pub struct Session {
    pub project: Project,
    /// The scene being edited, by id. Session state, not the document: selecting a scene is
    /// neither an edit nor an undo step, and doesn't make the project unsaved.
    pub active_scene: Option<String>,
    /// The last variation recalled with a morph (scene id, beats), for the engine: the document
    /// jumps to the variation, the render thread glides there. Whoever drives the engine takes it.
    pub morph: Option<(String, f32)>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// The top undo entry may still absorb edits with the same merge key (drags).
    merge_open: bool,
}

struct Entry {
    label: &'static str,
    merge_key: Option<String>,
    project: Project,
}

const MAX_UNDO: usize = 500;

impl Session {
    /// Starts fresh from a loaded project (history cleared).
    pub fn with_project(project: Project) -> Self {
        let active_scene = project.scenes.first().map(|s| s.id.clone());
        Self { project, active_scene, ..Default::default() }
    }

    /// The scene being edited, if it (still) exists.
    pub fn active(&self) -> Option<&Scene> {
        self.project.scene(self.active_scene.as_deref()?)
    }

    /// Runs an operator by id with JSON props.
    pub fn run(&mut self, id: &str, props: Value) -> Result<(), String> {
        self.exec(id, props, None)
    }

    /// Like [`Self::run`], but consecutive calls with the same `key` form one undo step,
    /// until [`Self::close_merge`]; used for continuous gestures like knob drags.
    pub fn run_merge(&mut self, id: &str, props: Value, key: &str) -> Result<(), String> {
        self.exec(id, props, Some(key.to_string()))
    }

    pub fn close_merge(&mut self) {
        self.merge_open = false;
    }

    fn exec(&mut self, id: &str, props: Value, merge_key: Option<String>) -> Result<(), String> {
        let op = find(id).ok_or_else(|| format!("unknown operator '{id}'"))?;
        if !(op.poll)(self) {
            return Err(format!("{} isn't possible right now", op.label));
        }
        let before = op.undo.then(|| self.project.clone());
        if let Err(e) = (op.exec)(self, props) {
            if let Some(b) = before {
                self.project = b;
            }
            return Err(e);
        }
        let Some(before) = before else { return Ok(()) };
        if before == self.project {
            return Ok(()); // no-op edits don't pollute history
        }
        let merging = merge_key.is_some() && self.merge_open && self.undo.last().is_some_and(|e| e.merge_key == merge_key);
        self.merge_open = merge_key.is_some();
        if !merging {
            self.undo.push(Entry { label: op.label, merge_key, project: before });
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        Ok(())
    }

    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|e| e.label)
    }

    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|e| e.label)
    }

    fn step(&mut self, back: bool) {
        let (from, to) = if back { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some(mut e) = from.pop() {
            std::mem::swap(&mut e.project, &mut self.project);
            e.merge_key = None;
            to.push(e);
        }
        self.merge_open = false;
        // the selected scene may be gone (undone add) or back (undone remove)
        if self.active().is_none() {
            self.active_scene = self.project.scenes.first().map(|s| s.id.clone());
        }
    }

    /// The scene named in props, or the active one (Blender-style "context").
    pub(crate) fn scene_index(&self, id: Option<&str>) -> Result<usize, String> {
        let id = id.or(self.active_scene.as_deref()).ok_or("no scene is selected")?;
        self.project.scenes.iter().position(|s| s.id == id).ok_or_else(|| format!("no scene {id}"))
    }

    fn scene_mut(&mut self, id: Option<&str>) -> Result<&mut Scene, String> {
        let i = self.scene_index(id)?;
        Ok(&mut self.project.scenes[i])
    }
}
