//! What runs live behind the UI: the engine (scene graphs built for it, launches, its preview
//! back as an egui texture) and the audio input. The UI thread never waits on either: graphs are
//! built on the compiler's thread, audio is opened and analysed on its own thread, and
//! everything comes back through queues and atomics.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use serde::{Deserialize, Serialize};
use velo_audio::{Analyzer, HOP, Mode, Settings, Source, Tempo, Tracker, apps, capture, synth};
use velo_core::{Curve, Launch, MACROS, ParamInfo, Project, Quantize, SOURCES, Scene, Transition, is_shader};
use velo_engine::media::Loading;
use velo_engine::shader::{Param, ParamType};
use velo_engine::{BUILTINS, Command, Compiler, Engine, MacroRoute, PREVIEW, Route, ShaderError, ShaderNode, SharedFeatures, SharedWave, macro_routes, routes};

use crate::host::Shared;

// ---------------------------------------------------------------------------------------------
// Shaders, as the inspector shows them

/// A shader's header, parsed once (it's pure and cheap), with a slider description per
/// parameter (two for a vec2).
pub struct Shader {
    pub controls: Vec<(&'static Param, Vec<ParamInfo>)>,
}

impl Shader {
    /// A parameter whose name starts with `_` is the engine's to set (a Media List's slot
    /// showing): no control.
    fn of(params: &'static [Param]) -> Self {
        Self { controls: params.iter().filter(|p| !p.id.starts_with('_')).map(|p| (p, infos(p))).collect() }
    }
}

static SHADERS: LazyLock<HashMap<&'static str, Shader>> = LazyLock::new(|| BUILTINS.iter().filter_map(|(id, src)| Some((*id, Shader::of(ShaderNode::parse(src).ok()?.params.leak())))).collect());

/// The signal nodes' parameters, shown as a shader's are: an LFO's shape by name and length in
/// beats, times in milliseconds ("attack_ms" is Attack).
static SIGNAL_SHADERS: LazyLock<HashMap<&'static str, Shader>> = LazyLock::new(|| {
    let kinds = velo_core::NODE_KINDS.iter().filter(|k| k.category == velo_core::Category::Signal);
    kinds
        .filter_map(|k| {
            let mut s = Shader::of(velo_engine::signal::params(k.id)?.leak());
            for (p, infos) in &mut s.controls {
                let (unit, name) = match p.id.as_str() {
                    "beats" => ("beats", None),
                    "seconds" => ("s", None),
                    id if id.ends_with("_ms") => ("ms", Some(&*label(id.trim_end_matches("_ms")).leak())),
                    id if id.ends_with("_low") || id.ends_with("_high") => ("Hz", None),
                    // a Band's edges
                    "low_hz" => ("Hz", Some("From")),
                    "high_hz" => ("Hz", Some("To")),
                    id if id.ends_with("_threshold") || id == "gate" => ("dB", None),
                    // the model's threshold is named as Simple mode's: one shows where the other hides
                    id if id.ends_with("_confidence") => ("%", Some(&*label(&id.replace("_confidence", "_threshold")).leak())),
                    _ => continue,
                };
                for i in infos.iter_mut() {
                    *i = ParamInfo { unit, name: name.unwrap_or(i.name), curve: if matches!(unit, "dB" | "%") { i.curve } else { Curve::Log }, ..*i };
                }
            }
            Some((k.id, s))
        })
        .collect()
});

/// A node's shader as the inspector shows it: a built-in's, a signal node's, or a Shader node's own,
/// from the header of its code as it is now (`None` while the header doesn't parse).
pub fn shader(kind: &str, code: Option<&str>) -> Option<&'static Shader> {
    if let Some(id) = velo_engine::builtin(kind) {
        return SHADERS.get(id);
    }
    if let Some(shader) = SIGNAL_SHADERS.get(kind) {
        return Some(shader);
    }
    let code = code.filter(|_| is_shader(kind))?;
    // ponytail: one leaked Shader per distinct header (a few hundred bytes, only while editing
    // the header); a cache with owned names if anyone ever edits headers for hours
    static OWN: LazyLock<Mutex<HashMap<String, &'static Shader>>> = LazyLock::new(Mutex::default);
    let header = code.split_once("}*/").map_or(code, |(h, _)| h);
    let mut own = OWN.lock().ok()?;
    if let Some(shader) = own.get(header) {
        return Some(shader);
    }
    let shader: &'static Shader = Box::leak(Box::new(Shader::of(velo_engine::own(code).ok()?.params.leak())));
    own.insert(header.to_string(), shader);
    Some(shader)
}

/// The knob for how much a node moves with the music by itself, no mapping needed: a shader's
/// `audio` (an effect preset's `music`), shown as Automagic.
pub fn automagic(id: &str) -> bool {
    id == "audio" || id == "music"
}

/// "color_a" → "Color a".
fn label(id: &str) -> String {
    let text = id.replace('_', " ");
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn infos(p: &Param) -> Vec<ParamInfo> {
    // ponytail: ParamInfo takes &'static str names; these are leaked once per built-in (~50
    // short strings). User shaders will need owned names.
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let curve = if p.ty == ParamType::Int { Curve::Steps } else { Curve::Linear };
    let info = |name: String, default: f32| ParamInfo { name: leak(name), min: p.min, max: p.max, default, curve, unit: "" };
    // an int with named values shows the name
    let named = (!p.names.is_empty()).then(|| leak(format!("|{}", p.names.join("|"))));
    // an angle (a turn across its range, a half turn at 1, or radians): in degrees, with a dial
    let span = p.max - p.min;
    let angle = p.ty == ParamType::Float && matches!(p.id.as_str(), "angle" | "direction" | "rotate") && [1.0, 2.0, std::f32::consts::TAU].iter().any(|s| (span - s).abs() < 1e-3);
    let info = |name, default| ParamInfo { unit: if angle { "°" } else { named.unwrap_or("") }, ..info(name, default) };
    match p.ty {
        ParamType::Vec2 => vec![info(format!("{} x", label(&p.id)), p.default[0]), info(format!("{} y", label(&p.id)), p.default[1])],
        ParamType::Bool => vec![ParamInfo { unit: "onoff", ..info(label(&p.id), p.default[0]) }],
        // edited by a node's own inspector (a Grid's cells), never as a slider
        ParamType::Vec4 => Vec::new(),
        _ if automagic(&p.id) => vec![info("Automagic".into(), p.default[0])],
        _ => vec![info(label(&p.id), p.default[0])],
    }
}

// ---------------------------------------------------------------------------------------------
// The engine: the scenes it holds, what goes out

/// Scene graphs kept built (docs/scenes.md §3, the warm cache): launching one of them back is
/// instant. More stay only while in use (live, fading out, launched, edited). Where the system
/// says how much video memory Velo may hold, as many as fit in half of it, at most [`WARM_MOST`];
/// elsewhere [`WARM`].
pub const WARM: usize = 4;
pub const WARM_MOST: usize = 12;

/// A scene's graph in the engine, or on its way there.
struct Slot {
    scene: String,
    /// What the engine knows it by.
    key: u64,
    /// The structure last sent to the compiler.
    asked: Option<u64>,
    /// Built: what the engine runs (each engine node's scene node and shader), the values and
    /// mappings last sent, and whether it has something to show.
    built: bool,
    installed: Vec<(String, ShaderNode)>,
    /// The graph's signal outputs ("n_ab12/0"), and its signal nodes' settings as last sent.
    signals: Vec<String>,
    kinds: Option<Vec<velo_engine::signal::Kind>>,
    sent: Vec<Vec<[f32; 4]>>,
    routed: Vec<Vec<Route>>,
    /// Its Image nodes' files (node id), and whether each was in when the graph was built.
    media: Vec<(String, Loading, bool)>,
    /// The macros' values and targets last sent (none: send them).
    macros: Option<[f32; MACROS]>,
    driving: Option<Vec<MacroRoute>>,
    /// Its engine node whose picture goes out (none: nothing does).
    output: Option<usize>,
    /// It holds a picture of what plays under it (a layer on top showing the program).
    under: bool,
}

/// The project's shared signal graph as the engine was last told: its outputs' names (what the
/// scenes' mappings of it route to), how its nodes are wired, their settings.
#[derive(Default, PartialEq)]
struct Signals {
    keys: Vec<String>,
    wiring: Vec<[Option<usize>; 2]>,
    kinds: Vec<velo_engine::signal::Kind>,
}

pub struct Live {
    pub engine: Rc<Engine>,
    pub shared: Rc<RefCell<Shared>>,
    /// The shared signal graph, as last sent.
    signals: Signals,
    compiler: Compiler,
    /// The scenes the engine holds or is getting, most recently used (edited, launched) first.
    slots: Vec<Slot>,
    /// The last key handed out.
    keys: u64,
    /// The last launch asked for, and the edited scene's graph, as the engine was told (0: none).
    launched: u64,
    edit: u64,
    /// The preview has something to show.
    showing: bool,
    /// A variation recalled with a morph, for the next sync: (scene, beats).
    morph: Option<(String, f32)>,
    pub preview: Option<egui::TextureHandle>,
    /// The edited scene's nodes, small, in a grid (the engine's [`velo_engine::runtime::THUMBS`]).
    thumbs: Option<egui::TextureHandle>,
    /// Each scene's picture as it last went out while it was the one edited (its card's).
    shots: HashMap<String, egui::TextureHandle>,
    /// The scene rendering a moment for its card's picture (one at a time), and the scenes
    /// that had their turn: one with nothing to show keeps its wash.
    peek: Option<Peek>,
    /// When the scene going out was last pictured for its card.
    shot_at: Instant,
    peeked: std::collections::HashSet<String>,
    /// Where the cards' pictures are kept between runs (`shots` in Velo's data folder), so a
    /// project opened again builds no scene for its card; nowhere in the tests.
    shot_dir: Option<PathBuf>,
    /// The video memory the system lets Velo hold, where it says (Windows), and the graphs held
    /// with what their pictures take, as of the last sync.
    pub budget: Option<u64>,
    pub held: (usize, u64),
    /// Layer scenes switched off (launching one switches it), and the layers as last sent.
    pub off: std::collections::HashSet<String>,
    layers: Vec<(u64, velo_core::Layer, velo_core::LayerBlend)>,
    pub blackout: bool,
    /// The output held on one frame (Freeze), and the test pattern on it instead.
    pub frozen: bool,
    pub pattern: bool,
    pub audio: Audio,
    /// Engine frames per second, measured over half a second; frames at the last measurement.
    pub fps: f32,
    counted: (Instant, u64),
    /// Since when the render thread hasn't finished a frame, if it hasn't for a while.
    still: Option<Instant>,
    /// The last tap, wherever it came from (the tempo chip flashes with it).
    pub tapped: std::cell::Cell<Option<Instant>>,
    /// Where the program is shared besides the outputs (Spout, NDI), if anywhere.
    pub sharing: Option<velo_engine::share::Share>,
    /// Shader nodes whose code didn't compile in their scene's latest build, by node id: they
    /// run their last good code meanwhile.
    pub errors: HashMap<String, ShaderError>,
    /// The preview's last pixels (RGBA, `PREVIEW` sized), kept while `keep_frame` (an agent is
    /// connected and may ask for a snapshot); the buffer is reused, no allocation per frame.
    pub frame: Vec<u8>,
    pub keep_frame: bool,
    /// Ableton Link, in builds with it.
    #[cfg(feature = "link")]
    link: crate::link::Link,
}

impl Live {
    pub fn new(engine: Rc<Engine>, shared: Rc<RefCell<Shared>>) -> Self {
        let compiler = Compiler::new(engine.device().clone(), engine.queue().clone(), engine.wave_texture().clone());
        let audio = Audio::new(engine.features.clone(), engine.wave.clone());
        let counted = (Instant::now(), 0);
        Self {
            engine,
            shared,
            signals: Signals::default(),
            compiler,
            slots: Vec::new(),
            keys: 0,
            launched: 0,
            edit: 0,
            showing: false,
            morph: None,
            preview: None,
            thumbs: None,
            shots: HashMap::new(),
            peek: None,
            shot_at: Instant::now(),
            peeked: Default::default(),
            // the tests have no data folder of their own: the user's is not theirs to write in
            shot_dir: if cfg!(test) { None } else { crate::files::data_dir().map(|d| d.join("shots")) },
            budget: None,
            held: (0, 0),
            off: Default::default(),
            layers: Vec::new(),
            blackout: false,
            frozen: false,
            pattern: false,
            audio,
            fps: 0.0,
            counted,
            still: None,
            tapped: std::cell::Cell::new(None),
            sharing: None,
            errors: HashMap::new(),
            frame: Vec::new(),
            keep_frame: false,
            #[cfg(feature = "link")]
            link: crate::link::Link::new(),
        }
    }

    /// Once a frame: the selected scene goes live if nothing ever did; scenes held are rebuilt
    /// when their structure changed and get changed values; finished builds are installed; the
    /// least recently used go; the newest preview is taken if `preview` is wanted. Returns a
    /// build error to show.
    pub fn sync(&mut self, ctx: &egui::Context, project: &Project, selected: Option<&Scene>, preview: bool) -> Option<String> {
        let size = [project.canvas.width, project.canvas.height];
        // the shared signals aren't a scene: while they're edited the preview shows the program
        let selected = selected.filter(|s| s.id != velo_core::SHARED);
        // Link's session leads the beat clock: its tempo, its place in the bar, every frame
        #[cfg(feature = "link")]
        if let Some((bpm, beat, at)) = self.link.frame(self.clock()[0]) {
            self.engine.send(Command::Follow { bpm, beat: Some(beat), at });
        }
        // at start, and after opening a project: the selected scene, at once; nothing launches
        // by itself after that
        if let Some(scene) = selected
            && self.launched == 0
        {
            self.launch_with(scene, Launch { transition: Transition::Cut, duration: 0.0, quantize: Quantize::Off });
        }
        // the edited scene is held too: the preview shows it
        let edit = selected.map_or(0, |s| self.slot(&s.id).key);
        if edit != self.edit {
            self.edit = edit;
            self.engine.send(Command::Edit(edit));
        }
        // a card with no picture yet: the one kept on disk from when this scene, as it is now,
        // was last pictured (nothing is built for it); else its scene held, built, rendered a
        // moment and pictured, and that picture kept
        while self.peek.is_none()
            && let Some(scene) = project.scenes.iter().find(|s| !self.shots.contains_key(&s.id) && !self.peeked.contains(&s.id))
        {
            let kept = self.shot_dir.as_deref().and_then(|dir| shot_file(dir, scene, size)).and_then(|file| load_shot(&file));
            if let Some(image) = kept {
                set_shot(&mut self.shots, ctx, &scene.id, image);
            } else {
                let key = self.slot(&scene.id).key;
                self.peek = Some(Peek { scene: scene.id.clone(), key, since: Instant::now(), sent: false, keep: true });
            }
        }
        // the scene going out: its card's picture again now and then, as it looks on air (the
        // edited one's comes with its nodes')
        let live = self.engine.status.live.load(Relaxed);
        if self.peek.is_none()
            && live != self.edit
            && self.shot_at.elapsed() > SHOT_EVERY
            && let Some(slot) = self.slots.iter().find(|s| s.key == live)
        {
            self.shot_at = Instant::now();
            self.peek = Some(Peek { scene: slot.scene.clone(), key: live, since: Instant::now(), sent: false, keep: false });
        }
        // the layers switched on, held and rendered every frame
        let on: Vec<&Scene> = project.scenes.iter().filter(|s| s.layer != velo_core::Layer::Scene && !self.off.contains(&s.id)).collect();
        let layers: Vec<_> = on.into_iter().map(|s| (self.slot(&s.id).key, s.layer, s.blend)).collect();
        if layers != self.layers {
            self.engine.send(Command::Layers(layers.clone()));
            self.layers = layers;
        }
        // a file decoded since its graph was built: built again, with it
        for slot in &mut self.slots {
            let mut fresh = false;
            for (_, loading, built) in &mut slot.media {
                if !*built && let Some(done) = loading.get() {
                    *built = true;
                    fresh |= done.is_ok();
                }
            }
            if fresh {
                slot.asked = None;
            }
        }
        for slot in &mut self.slots {
            let Some(scene) = project.scene(&slot.scene) else { continue };
            let shape = structure(Some(scene), size);
            if slot.asked != Some(shape) {
                slot.asked = Some(shape);
                self.compiler.build(slot.key, scene.clone(), size);
            }
        }
        let mut error = None;
        for (key, built) in self.compiler.built.try_iter() {
            // gone while it was building: dropped
            let Some(slot) = self.slots.iter_mut().find(|s| s.key == key) else { continue };
            match built {
                Ok(b) => {
                    slot.built = true;
                    slot.output = b.plan.output;
                    slot.under = !b.plan.under.is_empty();
                    self.errors.retain(|id, _| !b.plan.nodes.iter().any(|(n, _)| n == id));
                    self.errors.extend(b.errors);
                    slot.installed = b.plan.nodes.into_iter().map(|(id, _)| id).zip(b.shaders).collect();
                    (slot.signals, slot.kinds) = (b.signals, None);
                    slot.media = b.media;
                    // the build set the values it was asked with; anything newer goes below
                    slot.sent = vec![Vec::new(); slot.installed.len()];
                    slot.routed = vec![Vec::new(); slot.installed.len()];
                    (slot.macros, slot.driving) = (None, None);
                    self.engine.send(Command::Install { key, graph: b.graph });
                }
                Err(e) => error = Some(e),
            }
        }
        if let Some(p) = &mut self.peek {
            let slot = self.slots.iter().find(|s| s.key == p.key);
            let mut got = false;
            if p.sent {
                let shots = &mut self.shots;
                // named by the scene as it is now, which is what was just rendered
                let keep = self.shot_dir.as_deref().filter(|_| p.keep).zip(project.scene(&p.scene)).and_then(|(dir, scene)| shot_file(dir, scene, size));
                self.engine.take_shot(|rgba| {
                    use velo_engine::runtime::THUMB;
                    set_shot(shots, ctx, &p.scene, egui::ColorImage::from_rgba_premultiplied([THUMB[0] as usize, THUMB[1] as usize], rgba));
                    if let Some(file) = &keep {
                        keep_shot(file, rgba, SHOTS_KEPT);
                    }
                    got = true;
                });
            } else if slot.is_some_and(|s| s.built && s.output.is_some()) {
                (p.sent, p.since) = (true, Instant::now());
                self.engine.send(Command::Peek(p.key));
            }
            // pictured; or nothing to picture: no output, a build that fails, a graph that never shows
            let waits = slot.is_some_and(|s| !s.built || s.output.is_some()) && p.since.elapsed() < PEEK_WAIT;
            if got || !waits {
                self.peeked.insert(p.scene.clone());
                self.peek = None;
            }
        }
        // the shared signals: planned again when they're rewired (each node keeps what it was
        // doing, the engine carries it over), retuned when a setting changed. Sent before the
        // routes below, which follow their outputs to their new slots in the same batch
        let (plan, keys) = velo_engine::signal::plan_shared(&project.shared);
        let wiring: Vec<[Option<usize>; 2]> = plan.iter().map(|s| s.inputs).collect();
        let kinds: Vec<velo_engine::signal::Kind> = plan.iter().map(|s| s.kind).collect();
        if keys != self.signals.keys || wiring != self.signals.wiring {
            self.engine.send(Command::Shared { signals: plan, keys: keys.clone() });
        } else if kinds != self.signals.kinds {
            self.engine.send(Command::Signals { key: velo_engine::runtime::SHARED_KEY, kinds: kinds.clone() });
        }
        self.signals = Signals { keys, wiring, kinds };
        // the hot path: values and mappings only, no rebuild (drags, undo of a drag); the
        // engine computes the modulated values itself, for every graph it renders
        let morph = self.morph.take();
        for slot in &mut self.slots {
            let Some(scene) = project.scene(&slot.scene) else { continue };
            // the macros: what they drive first (a node they let go of gets its values again
            // below), then where they are, or a morph to there
            let nodes: Vec<(&str, &ShaderNode)> = slot.installed.iter().map(|(id, shader)| (id.as_str(), shader)).collect();
            let driving = macro_routes(&nodes, &scene.macros);
            if slot.driving.as_ref() != Some(&driving) {
                self.engine.send(Command::MacroTargets { key: slot.key, routes: driving.clone() });
                slot.driving = Some(driving);
                slot.sent.iter_mut().for_each(Vec::clear);
            }
            let values = scene.macros.each_ref().map(|m| m.value);
            match &morph {
                Some((id, beats)) if *id == scene.id && slot.built => self.engine.send(Command::Morph { key: slot.key, to: values, beats: *beats }),
                _ => {
                    for (index, &value) in values.iter().enumerate().filter(|(i, v)| slot.macros.is_none_or(|sent| sent[*i] != **v)) {
                        self.engine.send(Command::Macro { key: slot.key, index, value });
                    }
                }
            }
            slot.macros = Some(values);
            // the signal nodes' settings, when they changed (same order as the build's)
            let (signals, keys) = velo_engine::signal::plan(scene);
            let kinds: Vec<velo_engine::signal::Kind> = signals.into_iter().map(|s| s.kind).collect();
            if keys == slot.signals && slot.built && slot.kinds.as_ref() != Some(&kinds) {
                self.engine.send(Command::Signals { key: slot.key, kinds: kinds.clone() });
                slot.kinds = Some(kinds);
            }
            for (n, (id, shader)) in slot.installed.iter().enumerate() {
                let Some(node) = scene.node(id) else { continue };
                let values = shader.values(|p| velo_core::engine_param(scene, node, p));
                if slot.sent[n] != values {
                    self.engine.send(Command::Params { key: slot.key, node: n, values: values.clone() });
                    slot.sent[n] = values;
                }
                let routes = routes(shader, &node.mappings, &slot.signals, &self.signals.keys);
                if slot.routed[n] != routes {
                    self.engine.send(Command::Mappings { key: slot.key, node: n, routes: routes.clone() });
                    slot.routed[n] = routes;
                }
            }
        }
        let status = &self.engine.status;
        let (live, fading) = (status.live.load(Relaxed), status.fading.load(Relaxed));
        let in_use: Vec<u64> = [live, fading, status.armed().map_or(0, |a| a.0), self.launched, self.edit, self.peek.as_ref().map_or(0, |p| p.key)].into_iter().chain(self.layers.iter().map(|l| l.0)).collect();
        // what a graph's pictures take: a texture of the canvas each node, two for feedback, a
        // history's frames at a quarter of one each, and two pictures a buffer at its scale (in quarters)
        let texture = u64::from(size[0]) * u64::from(size[1]) * 8;
        let quarters = |s: &velo_engine::ShaderNode| {
            let buffers: u64 = s.buffers.iter().map(|b| (8.0 * b.scale * b.scale).ceil() as u64).sum();
            // a mesh's samples (a texture's worth each) and its depth (half a texture's a sample); its triangles aren't counted
            let mesh = if s.mesh { u64::from(velo_engine::gpu::MSAA) * 6 } else { 0 };
            4 * (1 + u64::from(s.feedback)) + if s.history { u64::from(velo_engine::gpu::HISTORY) } else { 0 } + buffers + mesh
        };
        // and a layer on top showing the program, one picture of what plays under it
        let bytes = |s: &Slot| (s.installed.iter().map(|(_, shader)| quarters(shader)).sum::<u64>() + 4 * u64::from(s.under)) * texture / 4;
        let held: Vec<(u64, bool, u64)> = self.slots.iter().map(|s| (s.key, project.scene(&s.scene).is_some(), bytes(s))).collect();
        let room = match self.budget.filter(|b| *b > 0) {
            // half of what the system lets Velo hold: the rest is the computer's
            Some(budget) => Room::Bytes(budget / 2),
            // fewer scenes kept warm as the canvas grows: four at 1080p, one at 4K
            None => Room::Graphs((WARM as u64 * 1920 * 1080 / (u64::from(size[0]) * u64::from(size[1])).max(1)).clamp(1, WARM as u64) as usize),
        };
        for key in evictions(&held, &in_use, room) {
            self.slots.retain(|s| s.key != key);
            self.engine.send(Command::Evict(key));
        }
        self.held = (self.slots.len(), self.slots.iter().map(bytes).sum());
        // the preview: the program while the edited scene is the live one, else the edited scene
        let shows = |key: u64| self.slots.iter().any(|s| s.key == key && s.built && s.output.is_some());
        self.showing = if self.edit == 0 || self.edit == live { shows(live) || shows(fading) } else { shows(self.edit) };
        if preview && self.showing {
            self.engine.take_preview(|rgba| {
                if self.keep_frame {
                    self.frame.clear();
                    self.frame.extend_from_slice(rgba);
                }
                let image = egui::ColorImage::from_rgba_premultiplied([PREVIEW[0] as usize, PREVIEW[1] as usize], rgba);
                match &mut self.preview {
                    Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                    None => self.preview = Some(ctx.load_texture("preview", image, egui::TextureOptions::LINEAR)),
                }
            });
        }
        // the cards' pictures, a few times a second
        let edited = self.slots.iter().find(|s| s.key == self.edit && s.built).and_then(|s| Some((s.scene.clone(), s.output?)));
        self.engine.take_thumbs(|rgba| {
            use velo_engine::runtime::{THUMB, THUMBS};
            let width = (THUMB[0] * THUMBS[0]) as usize;
            let image = egui::ColorImage::from_rgba_premultiplied([width, (THUMB[1] * THUMBS[1]) as usize], rgba);
            match &mut self.thumbs {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => self.thumbs = Some(ctx.load_texture("thumbs", image, egui::TextureOptions::LINEAR)),
            }
            // the edited scene's picture, out of its output's cell: its card's
            if let Some((scene, n)) = edited.filter(|(_, n)| (*n as u32) < THUMBS[0] * THUMBS[1]) {
                let (cell_w, cell_h) = (THUMB[0] as usize, THUMB[1] as usize);
                let (x0, y0) = ((n % THUMBS[0] as usize) * cell_w, (n / THUMBS[0] as usize) * cell_h);
                let mut cell = Vec::with_capacity(cell_w * cell_h * 4);
                for y in y0..y0 + cell_h {
                    cell.extend_from_slice(&rgba[(y * width + x0) * 4..(y * width + x0 + cell_w) * 4]);
                }
                set_shot(&mut self.shots, ctx, &scene, egui::ColorImage::from_rgba_premultiplied([cell_w, cell_h], &cell));
            }
        });
        let (at, frames) = self.counted;
        if at.elapsed() >= Duration::from_millis(500) {
            let now = self.engine.status.frames.load(Relaxed);
            self.fps = (now - frames) as f32 / at.elapsed().as_secs_f32();
            self.counted = (Instant::now(), now);
            self.still = if now == frames { self.still.or(Some(at)) } else { None };
        }
        error
    }

    /// A scene's slot, made the most recently used; a new one is built by the next sync.
    fn slot(&mut self, scene: &str) -> &mut Slot {
        let slot = match self.slots.iter().position(|s| s.scene == scene) {
            Some(i) => self.slots.remove(i),
            None => {
                self.keys += 1;
                Slot {
                    scene: scene.to_string(),
                    key: self.keys,
                    asked: None,
                    built: false,
                    installed: Vec::new(),
                    signals: Vec::new(),
                    kinds: None,
                    sent: Vec::new(),
                    routed: Vec::new(),
                    media: Vec::new(),
                    macros: None,
                    driving: None,
                    output: None,
                    under: false,
                }
            }
        };
        self.slots.insert(0, slot);
        &mut self.slots[0]
    }

    /// Puts `scene` on the output with its launch settings: on its quantize boundary, once its
    /// graph is built (now, if it's held).
    /// Shares the program with other apps (Spout) or over the network (NDI), or stops.
    pub fn share(&mut self, kind: Option<velo_engine::share::Share>) {
        self.sharing = kind;
        self.engine.send(Command::Share(kind));
    }

    /// The engine can't go on: its GPU was lost, or its render thread hasn't finished a frame
    /// in 2 s (stuck, or gone).
    pub fn broken(&self) -> bool {
        self.engine.status.lost.load(Relaxed) || self.still.is_some_and(|at| at.elapsed() > Duration::from_secs(2))
    }

    /// `scene` out at once (a restarted engine takes up the show where it was).
    pub fn resume(&mut self, scene: &Scene) {
        self.launch_with(scene, Launch { transition: Transition::Cut, duration: 0.0, quantize: Quantize::Off });
    }

    /// Puts `scene` on the output with its launch settings; a layer is switched on or off.
    pub fn launch(&mut self, scene: &Scene) {
        if scene.layer != velo_core::Layer::Scene {
            if !self.off.remove(&scene.id) {
                self.off.insert(scene.id.clone());
            }
            return;
        }
        self.launch_with(scene, scene.launch);
    }

    /// Puts `scene` on the output `launch`'s way instead of its own.
    pub fn launch_with(&mut self, scene: &Scene, launch: Launch) {
        // the shared signals run whatever goes out: never on the output themselves
        if scene.id == velo_core::SHARED {
            return;
        }
        let key = self.slot(&scene.id).key;
        self.launched = key;
        self.engine.send(Command::Launch { scene: key, transition: launch.transition, duration_beats: launch.duration, quantize: launch.quantize });
    }

    /// Another project: its selected scene goes live with the next sync.
    pub fn restart(&mut self) {
        self.launched = 0;
    }

    fn scene_of(&self, key: u64) -> Option<&str> {
        self.slots.iter().find(|s| s.key == key && key != 0).map(|s| s.scene.as_str())
    }

    /// The scene on the output: the incoming one, once a crossfade starts.
    pub fn live_scene(&self) -> Option<&str> {
        self.scene_of(self.engine.status.live.load(Relaxed))
    }

    /// How far along its wait for the boundary a launched scene is (0..1; 0 while it builds).
    pub fn armed(&self, scene: &str) -> Option<f32> {
        self.engine.status.armed().filter(|(key, _)| self.scene_of(*key) == Some(scene)).map(|a| a.1)
    }

    /// How many of the project's scenes have their card's picture, or can't have one. Each is
    /// built once to be pictured (its shaders compiled, its files opened), which is what makes a
    /// project slow right after it opens: the loading screen waits for them all.
    pub fn pictured(&self, project: &Project) -> usize {
        project.scenes.iter().filter(|s| self.shots.contains_key(&s.id) || self.peeked.contains(&s.id)).count()
    }

    /// Started, and every scene it holds built (the splash waits for it).
    pub fn settled(&self) -> bool {
        self.launched > 0 && self.slots.iter().all(|s| s.built)
    }

    /// The preview texture, when there's something to show.
    pub fn picture(&self) -> Option<egui::TextureId> {
        self.preview.as_ref().filter(|_| self.showing).map(egui::TextureHandle::id)
    }

    /// A scene's picture as it last went out while it was edited (its card's), if it has been.
    pub fn shot(&self, scene: &str) -> Option<egui::TextureId> {
        self.shots.get(scene).map(egui::TextureHandle::id)
    }

    /// A node of the edited scene as the engine drew it lately: the grid's texture and the
    /// node's cell in it (none for a node that isn't a program: an Output, an LFO).
    pub fn thumb(&self, node: &str) -> Option<(egui::TextureId, egui::Rect)> {
        use velo_engine::runtime::THUMBS;
        let slot = self.slots.iter().find(|s| s.key == self.edit && s.built)?;
        let n = slot.installed.iter().position(|(id, _)| id == node)? as u32;
        if n >= THUMBS[0] * THUMBS[1] {
            return None;
        }
        let (cols, rows) = (THUMBS[0] as f32, THUMBS[1] as f32);
        let min = egui::pos2((n % THUMBS[0]) as f32 / cols, (n / THUMBS[0]) as f32 / rows);
        Some((self.thumbs.as_ref()?.id(), egui::Rect::from_min_size(min, egui::vec2(1.0 / cols, 1.0 / rows))))
    }

    /// What a node of the edited scene cost the GPU lately, in ms, where the GPU says.
    pub fn cost(&self, node: &str) -> Option<f32> {
        let status = &self.engine.status;
        if status.costed.load(Relaxed) != self.edit {
            return None;
        }
        let slot = self.slots.iter().find(|s| s.key == self.edit && s.built)?;
        let n = slot.installed.iter().position(|(id, _)| id == node)?;
        status.costs.0.get(n).map(|v| f32::from_bits(v.load(Relaxed)))
    }

    /// Whether a node of the edited scene sleeps: nothing it feeds shows now (a Switch's other
    /// inputs, a Blend's at no opacity), so it keeps its last picture.
    pub fn asleep(&self, node: &str) -> bool {
        let Some(slot) = self.slots.iter().find(|s| s.key == self.edit && s.built) else { return false };
        slot.installed.iter().position(|(id, _)| id == node).is_some_and(|n| n < 64 && self.engine.status.asleep.load(Relaxed) >> n & 1 == 1)
    }

    /// A node of the edited scene, with its values as the engine modulated them last frame (x
    /// and y per parameter, in header order), while it's mapped; asking makes the engine
    /// publish them.
    pub fn modulated(&self, node: &str) -> Option<[f32; 32]> {
        let slot = self.slots.iter().find(|s| s.key == self.edit)?;
        let n = slot.installed.iter().position(|(id, _)| id == node)?;
        self.engine.status.watch(n)
    }

    /// What a signal node's output reads now: in the shared signals (node ids are unique in the
    /// project), else in the edited scene's graph (its card's meter).
    pub fn signal(&self, node: &str, port: u8) -> Option<f32> {
        let key = format!("{node}/{port}");
        if let Some(at) = self.signals.keys.iter().position(|k| *k == key) {
            return Some(self.engine.status.shared(at));
        }
        let slot = self.slots.iter().find(|s| s.key == self.edit)?;
        let at = slot.signals.iter().position(|k| *k == key)?;
        Some(self.engine.status.signal(at))
    }

    /// What a mapping's source reads now: an output of the edited scene's signal nodes or of the
    /// shared signals, or a source by name ("kick"); 0 for one that's gone.
    pub fn level(&self, source: &str) -> f32 {
        match velo_core::shared_output(source).or_else(|| velo_core::signal_output(source)) {
            Some((id, port)) => self.signal(id, port).unwrap_or(0.0),
            None => SOURCES.iter().position(|s| *s == source).map_or(0.0, |i| self.sources()[i]),
        }
    }

    /// What every source reads now, in `velo_core::SOURCES`' order: the audio features, then
    /// the beat's hit and the ramp through the bar (the chips' meters, the badges' glow).
    pub fn sources(&self) -> [f32; SOURCES.len()] {
        let (features, [_, beat, bar]) = (self.engine.features.load(), self.clock());
        let mut s = [0.0; SOURCES.len()];
        s[..7].copy_from_slice(&features[..7]);
        s[7..].copy_from_slice(&[beat, bar, features[11]]);
        s
    }

    /// An Image node's file as the engine has it: loading (empty), decoded, or why not.
    pub fn media(&self, node: &str) -> Option<Loading> {
        self.slots.iter().flat_map(|s| &s.media).find(|m| m.0 == node).map(|m| m.1.clone())
    }

    /// Beats since the beat clock's beat 0 (what LFOs run on).
    pub fn beats(&self) -> f64 {
        f64::from_bits(self.engine.status.beats.load(Relaxed))
    }

    /// The beat clock: bpm, the beat's hit (0..1), the ramp through the bar (0..1).
    pub fn clock(&self) -> [f32; 3] {
        self.engine.status.clock()
    }

    pub fn tap(&self) {
        self.tap_at(Instant::now());
    }

    /// A tap that happened at `at` (a MIDI pad's, stamped when it arrived).
    pub fn tap_at(&self, at: Instant) {
        self.tapped.set(Some(Instant::now()));
        self.engine.send(Command::Tap(at));
        #[cfg(feature = "link")]
        self.link.tapped();
    }

    /// The next sync glides `scene`'s macros to where the document has them over `beats`,
    /// on the engine's beat clock, instead of setting them at once.
    pub fn morph(&mut self, scene: String, beats: f32) {
        self.morph = Some((scene, beats));
    }

    pub fn set_bpm(&self, bpm: f32) {
        self.engine.send(Command::Tempo(bpm));
        #[cfg(feature = "link")]
        self.link.set_tempo(bpm);
    }

    /// An outside clock's beat (MIDI clock, the music's tempo), as it came: Link's session
    /// takes its tempo too.
    pub fn follow(&self, bpm: f32, beat: Option<f64>, at: Instant) {
        self.engine.send(Command::Follow { bpm, beat, at });
        #[cfg(feature = "link")]
        self.link.set_tempo(bpm);
    }

    /// Ableton Link on or off (in builds with it).
    #[cfg_attr(not(feature = "link"), allow(unused_variables))]
    pub fn set_link(&self, on: bool) {
        #[cfg(feature = "link")]
        self.link.enable(on, self.clock()[0], self.beats());
    }

    /// Link's peers while it's on; none while it's off, or in builds without it.
    pub fn link_peers(&self) -> Option<u64> {
        #[cfg(feature = "link")]
        return self.link.peers();
        #[cfg(not(feature = "link"))]
        None
    }

    /// The pictures drawn `ms` ahead of the music, for a projector that shows them that late.
    pub fn set_lead(&self, ms: f32) {
        self.engine.send(Command::Lead(ms.max(0.0) / 1000.0));
    }

    /// A downbeat at `at` (a MIDI clock's Start, as it came).
    pub fn resync_at(&self, at: Instant) {
        self.engine.send(Command::Resync(at));
    }

    /// The next downbeat is now.
    pub fn resync(&self) {
        self.engine.send(Command::Resync(Instant::now()));
    }

    pub fn set_blackout(&mut self, on: bool) {
        self.blackout = on;
        self.engine.send(Command::Blackout(on));
    }

    pub fn set_freeze(&mut self, on: bool) {
        self.frozen = on;
        self.engine.send(Command::Freeze(on));
    }

    pub fn set_pattern(&mut self, on: bool) {
        self.pattern = on;
        self.engine.send(Command::Pattern(on));
    }
}

/// A scene rendering a moment for its card's picture: since when it waits (to be built, then
/// for the picture).
struct Peek {
    scene: String,
    key: u64,
    since: Instant,
    sent: bool,
    /// A card's first picture, to keep on disk; not the scene going out's, taken again every
    /// two seconds.
    keep: bool,
}

/// How many cards' pictures are kept on disk (about 10 KB each): past it, the least recently
/// used go.
const SHOTS_KEPT: usize = 400;

/// The file in `dir` a scene's card's picture is kept in: named by a hash of all the scene is
/// (its nodes with their values, mappings, code, files and words, its wires, its macros) and of
/// the canvas size, so a scene changed in any way has another name and is pictured again.
fn shot_file(dir: &Path, scene: &Scene, size: [u32; 2]) -> Option<PathBuf> {
    let text = ron::to_string(&(scene, size)).ok()?;
    // FNV-1a: the same in every version of Velo, as a file's name must be (std's hasher
    // doesn't promise that)
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    Some(dir.join(format!("{hash:016x}.png")))
}

/// The picture kept in `file`, if there's one and it's a card's; read, it's the last used.
fn load_shot(file: &Path) -> Option<egui::ColorImage> {
    use velo_engine::runtime::THUMB;
    let picture = image::open(file).ok()?.into_rgba8();
    if picture.dimensions() != (THUMB[0], THUMB[1]) {
        return None;
    }
    let _ = std::fs::File::options().write(true).open(file).and_then(|f| f.set_modified(SystemTime::now()));
    Some(egui::ColorImage::from_rgba_premultiplied([THUMB[0] as usize, THUMB[1] as usize], &picture))
}

/// Keeps a card's picture (as the engine gives it, premultiplied) in `file`, and no more than
/// `most` pictures in its folder: the least recently used go. One that can't be written is
/// pictured again at the next open, which is all a failure costs.
fn keep_shot(file: &Path, rgba: &[u8], most: usize) {
    use velo_engine::runtime::THUMB;
    let Some(dir) = file.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() || image::save_buffer(file, rgba, THUMB[0], THUMB[1], image::ColorType::Rgba8).is_err() {
        return;
    }
    let pictures = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "png"));
    let mut kept: Vec<(SystemTime, PathBuf)> = pictures.filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p))).collect();
    kept.sort();
    for (_, old) in kept.iter().rev().skip(most) {
        let _ = std::fs::remove_file(old);
    }
}

/// How long a card's picture may take to build, and then to come, before its scene's turn ends.
const PEEK_WAIT: Duration = Duration::from_secs(10);

/// How often the scene going out is pictured again for its card.
const SHOT_EVERY: Duration = Duration::from_secs(2);

/// A scene's card's picture, new or replaced.
fn set_shot(shots: &mut HashMap<String, egui::TextureHandle>, ctx: &egui::Context, scene: &str, image: egui::ColorImage) {
    match shots.get_mut(scene) {
        Some(t) => t.set(image, egui::TextureOptions::LINEAR),
        None => {
            shots.insert(scene.to_string(), ctx.load_texture(format!("shot {scene}"), image, egui::TextureOptions::LINEAR));
        }
    }
}

/// How much the warm cache may hold, the graphs in use included: so many graphs, or so many
/// bytes of pictures (and [`WARM_MOST`] graphs).
#[derive(Clone, Copy, Debug)]
enum Room {
    Graphs(usize),
    Bytes(u64),
}

/// The warm cache's evictions: given the held graphs most recently used first (each with
/// whether its scene still exists and what its pictures take), the ones to drop, least recently
/// used first. A deleted scene's goes at once, then the least recently used past `room`; never
/// one in use.
fn evictions(held: &[(u64, bool, u64)], in_use: &[u64], room: Room) -> Vec<u64> {
    let used = held.iter().filter(|h| in_use.contains(&h.0));
    let (mut kept, mut taken) = (used.clone().count(), used.map(|h| h.2).sum::<u64>());
    let mut out = Vec::new();
    for &(key, exists, bytes) in held.iter().filter(|h| !in_use.contains(&h.0)) {
        let fits = match room {
            Room::Graphs(most) => kept < most,
            Room::Bytes(most) => kept < WARM_MOST && taken + bytes <= most,
        };
        if exists && fits {
            (kept, taken) = (kept + 1, taken + bytes);
        } else {
            out.push(key);
        }
    }
    out.reverse();
    out
}

/// A hash of what needs a rebuild when it changes: the scene, its nodes' kinds, its wires,
/// the canvas size. Positions, names and values don't.
fn structure(scene: Option<&Scene>, size: [u32; 2]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    size.hash(&mut h);
    if let Some(s) = scene {
        s.id.hash(&mut h);
        for n in &s.nodes {
            (&n.id, &n.kind, &n.code, &n.media, &n.text, n.off).hash(&mut h);
        }
        // a layer on top is built with the picture under it, for its Live nodes on the program
        (s.layer == velo_core::Layer::Top && s.nodes.iter().any(|n| n.media.as_deref() == Some(velo_core::LIVE_PROGRAM))).hash(&mut h);
        for e in &s.edges {
            (&e.from, &e.to).hash(&mut h);
        }
    }
    h.finish()
}

// ---------------------------------------------------------------------------------------------
// Audio

/// Which input to listen to; remembered per machine (D14).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Input {
    /// Whatever the computer plays (D14: the first-run default).
    #[default]
    Computer,
    /// The system's default input device.
    Default,
    Named(String),
    /// One app's sound (macOS 14.2+): its bundle on disk, its name.
    App {
        id: String,
        name: String,
    },
}

/// How the input is doing, for the top bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Hearing {
    /// Opening, or open with nothing heard yet (on macOS the permission prompt holds it).
    #[default]
    Waiting,
    Live,
    /// Open for a while and nothing but exact zeros: no permission, most likely.
    Nothing,
    Failed(String),
}

#[derive(Clone, Debug, Default)]
pub struct AudioState {
    pub name: String,
    pub hearing: Hearing,
    /// Input devices, for the menu.
    pub devices: Vec<String>,
    /// Apps that use audio, for the menu, every couple of seconds.
    pub apps: Vec<apps::App>,
    /// The music's tempo as last heard (auto-BPM), once a second.
    pub tempo: Option<Heard>,
}

/// A tempo estimate from the audio: how sure, and when one of its beats was.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub bpm: f32,
    pub confidence: f32,
    pub beat: Instant,
}

/// Runs auto-BPM on a hop; a new estimate goes into the state, its beat as an `Instant`.
fn hear_tempo(tempo: &mut Tempo, hop: &velo_audio::Hop, state: &Mutex<AudioState>) {
    if let Some(e) = tempo.push(hop.strength, hop.heard)
        && let Ok(mut s) = state.lock()
    {
        let ago = Duration::from_secs_f64((hop.time - e.beat_at).max(0.0));
        s.tempo = Some(Heard { bpm: e.bpm, confidence: e.confidence, beat: Instant::now() - ago });
    }
}

/// An Audio In's detection settings ([`Settings`]) from its parameters (velo_engine::signal's
/// ids); the defaults for none.
pub fn detection(node: Option<&velo_core::Node>) -> Settings {
    let d = Settings::default();
    let Some(node) = node else { return d };
    let get = |p: &str, default: f32| node.params.get(p).map_or(default, |v| v[0]);
    let mode = if get("mode", if d.mode == Mode::Model { 1.0 } else { 0.0 }).round() >= 1.0 { Mode::Model } else { Mode::Simple };
    let detect = std::array::from_fn(|c| {
        let (name, x) = (velo_audio::HITS[c], d.detect[c]);
        velo_audio::Detect {
            threshold_db: get(&format!("{name}_threshold"), x.threshold_db),
            low_hz: get(&format!("{name}_low"), x.low_hz),
            high_hz: get(&format!("{name}_high"), x.high_hz),
            gap_s: get(&format!("{name}_gap_ms"), x.gap_s * 1000.0) / 1000.0,
        }
    });
    let model = std::array::from_fn(|c| get(&format!("{}_confidence", velo_audio::HITS[c]), d.model[c]));
    Settings { mode, gate_db: get("gate", d.gate_db), detect, model }
}

/// The detection settings on their way to the audio thread, without a lock: each value's bits,
/// and whether they changed since it last took them.
#[derive(Default)]
struct Tuning {
    values: [AtomicU32; 17],
    changed: AtomicBool,
}

impl Tuning {
    fn store(&self, s: &Settings) {
        let mode = if s.mode == Mode::Model { 1.0 } else { 0.0 };
        let values = [mode, s.gate_db].into_iter().chain(s.detect.iter().flat_map(|d| [d.threshold_db, d.low_hz, d.high_hz, d.gap_s])).chain(s.model);
        for (slot, v) in self.values.iter().zip(values) {
            slot.store(v.to_bits(), Relaxed);
        }
        self.changed.store(true, Release);
    }

    fn take(&self) -> Option<Settings> {
        if !self.changed.swap(false, Acquire) {
            return None;
        }
        let v: [f32; 17] = std::array::from_fn(|i| f32::from_bits(self.values[i].load(Relaxed)));
        let detect = std::array::from_fn(|c| velo_audio::Detect { threshold_db: v[2 + 4 * c], low_hz: v[3 + 4 * c], high_hz: v[4 + 4 * c], gap_s: v[5 + 4 * c] });
        Some(Settings { mode: if v[0] >= 0.5 { Mode::Model } else { Mode::Simple }, gate_db: v[1], detect, model: [v[14], v[15], v[16]] })
    }
}

/// What each detector watched since the UI last looked (the highest, as bits: never negative,
/// so they order as the numbers do), and whether it fired.
#[derive(Default)]
struct Meter {
    signals: [AtomicU32; 3],
    hits: [AtomicBool; 3],
    /// The loudest level (dB, 200 over, so its bits order as the numbers do).
    level: AtomicU32,
    /// The same, for the autopilot's ear on silence, which looks every frame.
    loudest: AtomicU32,
}

impl Meter {
    fn record(&self, hop: &velo_audio::Hop) {
        self.level.fetch_max((hop.level_db + 200.0).max(0.0).to_bits(), Relaxed);
        self.loudest.fetch_max((hop.level_db + 200.0).max(0.0).to_bits(), Relaxed);
        for c in 0..3 {
            let s = hop.signals[c];
            self.signals[c].fetch_max(if s > 0.0 { s.to_bits() } else { 0 }, Relaxed);
            if hop.onsets[c].is_some() {
                self.hits[c].store(true, Relaxed);
            }
        }
    }
}

/// What the audio thread shares with the others, all without a lock: features for the render
/// thread, samples for the audio texture, the detectors' settings in and what they watch out.
#[derive(Clone)]
struct Links {
    features: Arc<SharedFeatures>,
    wave: Arc<SharedWave>,
    tuning: Arc<Tuning>,
    meter: Arc<Meter>,
}

/// Seconds of the detectors' signals the Audio In's graphs show.
pub const GRAPH_S: f64 = 3.0;

/// The audio thread from the UI's side. One thread per input: switching stops the old one.
pub struct Audio {
    links: Links,
    stop: Arc<AtomicBool>,
    pub state: Arc<Mutex<AudioState>>,
    pub input: Option<Input>,
    /// The detection settings last sent.
    pub settings: Settings,
    /// Each detector's signal over the last [`GRAPH_S`]: when (UI seconds), the highest since the
    /// sample before, whether it fired.
    pub graphs: [VecDeque<(f64, f32, bool)>; 3],
    /// Whether the input stayed under the gate since the sample before: no hit could fire.
    pub gated: bool,
}

impl Audio {
    fn new(features: Arc<SharedFeatures>, wave: Arc<SharedWave>) -> Self {
        let links = Links { features, wave, tuning: Arc::default(), meter: Arc::default() };
        Self { links, stop: Arc::new(AtomicBool::new(true)), state: Arc::default(), input: None, settings: Settings::default(), graphs: Default::default(), gated: false }
    }

    /// The input to listen to at start: the one chosen, or the test loop (`VELO_AUDIO=synth`).
    pub fn first_input(chosen: &Input) -> Option<Input> {
        (std::env::var("VELO_AUDIO").as_deref() != Ok("synth")).then(|| chosen.clone())
    }

    /// Listens to `input`, or to the synthetic test loop with `None` (`VELO_AUDIO=synth`).
    pub fn listen(&mut self, input: Option<Input>) {
        self.stop.store(true, Relaxed);
        let (devices, apps) = self.state.lock().map(|s| (s.devices.clone(), s.apps.clone())).unwrap_or_default();
        self.stop = Arc::new(AtomicBool::new(false));
        self.state = Arc::new(Mutex::new(AudioState { devices, apps, ..Default::default() }));
        self.input = input.clone();
        // the new thread starts from the settings as they are
        self.links.tuning.store(&self.settings);
        let (links, stop, state) = (self.links.clone(), self.stop.clone(), self.state.clone());
        let _ = std::thread::Builder::new().name("velo audio".into()).spawn(move || listen(input, &links, &stop, &state));
    }

    /// The state as last published; never waits for the audio thread.
    pub fn state(&self) -> Option<AudioState> {
        self.state.try_lock().ok().map(|s| s.clone())
    }

    /// New detection settings, sent only when they changed.
    pub fn tune(&mut self, settings: Settings) {
        if settings != self.settings {
            self.settings = settings;
            self.links.tuning.store(&settings);
        }
    }

    /// The detectors' signals since the last sample into [`Self::graphs`], at most every 1/60 s
    /// (`now` in seconds): what the Audio In's graphs draw.
    pub fn sample_graphs(&mut self, now: f64) {
        if self.graphs[0].back().is_some_and(|g| now - g.0 < 1.0 / 60.0) {
            return;
        }
        for (c, graph) in self.graphs.iter_mut().enumerate() {
            let signal = f32::from_bits(self.links.meter.signals[c].swap(0, Relaxed));
            graph.push_back((now, signal, self.links.meter.hits[c].swap(false, Relaxed)));
            while graph.front().is_some_and(|g| now - g.0 > GRAPH_S) {
                graph.pop_front();
            }
        }
        self.gated = f32::from_bits(self.links.meter.level.swap(0, Relaxed)) - 200.0 <= self.settings.gate_db;
    }

    /// Whether everything heard since the last ask stayed under the gate (nothing heard at all,
    /// no input or a stalled one, is silence too): what the autopilot's silence scene waits on.
    pub fn silent(&self) -> bool {
        f32::from_bits(self.links.meter.loudest.swap(0, Relaxed)) - 200.0 <= self.settings.gate_db
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

/// How long an open input may deliver only zeros before we say so.
const NOTHING_AFTER: Duration = Duration::from_secs(2);

/// How often the audio thread looks at which apps use audio (and an app's input at its processes).
const APPS_EVERY: Duration = Duration::from_secs(2);

/// The audio thread: opens the input (this may block on the macOS permission prompt), then
/// drains the capture ring into the analyzer and publishes the features for the render thread.
/// One app's input waits for the app to open, and follows it when it opens again.
fn listen(input: Option<Input>, links: &Links, stop: &AtomicBool, state: &Mutex<AudioState>) {
    let set = |f: &dyn Fn(&mut AudioState)| {
        if let Ok(mut s) = state.lock() {
            f(&mut s);
        }
    };
    let Some(input) = input else { return test_loop(links, stop, state) };
    let devices = capture::inputs();
    set(&|s| s.devices = devices.clone());
    let source = match &input {
        Input::Computer => Source::Computer,
        Input::Default => Source::DefaultInput,
        Input::Named(name) => Source::Named(name.clone()),
        Input::App { id, name } => Source::App { id: id.clone(), name: name.clone() },
    };
    let app = match &input {
        Input::App { id, .. } => Some(id.as_str()),
        _ => None,
    };
    let mut looked = None;
    // an input that goes (unplugged, disabled) is asked for again until it's back
    while !stop.load(Relaxed) {
        look_at_apps(&mut looked, state);
        match capture::open(&source) {
            Ok(mut cap) => {
                set(&|s| (s.name, s.hearing) = (cap.name.clone(), Hearing::Waiting));
                hear(&mut cap, &source, app, &mut looked, links, stop, state);
                if cap.broken.load(Relaxed) {
                    set(&|s| s.hearing = Hearing::Failed(tr!("{} went away: waiting for it to come back", cap.name)));
                }
            }
            Err(e) => set(&|s| s.hearing = Hearing::Failed(e.clone())),
        }
        // what the mappings follow lets go while nothing is heard
        links.features.store([0.0; 12]);
        std::thread::sleep(Duration::from_millis(if app.is_some() { 250 } else { 1000 }));
    }
    links.features.store([0.0; 12]);
}

/// The apps using audio, for the menu, when it's been a while.
fn look_at_apps(looked: &mut Option<Instant>, state: &Mutex<AudioState>) -> bool {
    if looked.is_some_and(|at| at.elapsed() < APPS_EVERY) {
        return false;
    }
    *looked = Some(Instant::now());
    let found = apps::apps();
    if let Ok(mut s) = state.lock() {
        s.apps = found;
    }
    true
}

/// An open input into the analyzer until stop, or until the app it listens to has other
/// processes (it quit, opened again, started a helper): then it's opened again.
fn hear(cap: &mut capture::Capture, source: &Source, app: Option<&str>, looked: &mut Option<Instant>, links: &Links, stop: &AtomicBool, state: &Mutex<AudioState>) {
    let set = |f: &dyn Fn(&mut AudioState)| {
        if let Ok(mut s) = state.lock() {
            f(&mut s);
        }
    };
    let (mut analyzer, mut tracker, mut tempo) = (Analyzer::new(cap.rate), Tracker::new(), Tempo::new(cap.rate));
    let dt = HOP as f32 / cap.rate;
    let (opened, mut heard) = (Instant::now(), false);
    // when samples last went through, and when the default device was last looked at
    let (mut fed, mut checked) = (Instant::now(), Instant::now());
    let mut buffer = vec![0.0f32; 4096];
    // a new analyzer: the settings as they are
    links.tuning.changed.store(true, Release);
    while !stop.load(Relaxed) {
        // the device went (open it again), or the default moved to another (hear that one)
        if cap.broken.load(Relaxed) {
            return;
        }
        if checked.elapsed() > Duration::from_secs(2) {
            checked = Instant::now();
            if cap.default_moved(source) {
                return;
            }
        }
        if let Some(s) = links.tuning.take() {
            analyzer.set(s);
        }
        if look_at_apps(looked, state)
            && let (Some(id), Some(tap)) = (app, &cap.tap)
        {
            let mut now = apps::processes(id);
            now.sort_unstable();
            let mut before = tap.processes.clone();
            before.sort_unstable();
            if now != before {
                return;
            }
        }
        let take = cap.samples.slots().min(buffer.len());
        if take == 0 {
            if !heard && opened.elapsed() > NOTHING_AFTER {
                set(&|s| s.hearing = Hearing::Nothing);
            }
            // no samples (nothing plays into a loopback, a device stalled): silence goes through
            // instead, so hits and levels fall back and nothing a mapping moves stays stuck up
            if fed.elapsed() > Duration::from_millis(50) {
                let silence = ((fed.elapsed().as_secs_f32() * cap.rate) as usize).min(buffer.len());
                buffer[..silence].fill(0.0);
                links.wave.push_samples(&buffer[..silence]);
                analyzer.push(&buffer[..silence], |hop| hop_out(hop, &mut tracker, dt, &mut tempo, links, state));
                links.wave.push_spectrum(analyzer.spectrum(), analyzer.rate());
                fed = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        fed = Instant::now();
        let Ok(chunk) = cap.samples.read_chunk(take) else { continue };
        let (a, b) = chunk.as_slices();
        buffer[..a.len()].copy_from_slice(a);
        buffer[a.len()..take].copy_from_slice(b);
        chunk.commit_all();
        if !heard && buffer[..take].iter().any(|&x| x != 0.0) {
            heard = true;
            set(&|s| s.hearing = Hearing::Live);
        } else if !heard && opened.elapsed() > NOTHING_AFTER {
            set(&|s| s.hearing = Hearing::Nothing);
        }
        links.wave.push_samples(&buffer[..take]);
        analyzer.push(&buffer[..take], |hop| hop_out(hop, &mut tracker, dt, &mut tempo, links, state));
        links.wave.push_spectrum(analyzer.spectrum(), analyzer.rate());
    }
}

/// `VELO_AUDIO=synth`: the test loop played in real time, for tests and screenshots without
/// a device or a permission.
fn test_loop(links: &Links, stop: &AtomicBool, state: &Mutex<AudioState>) {
    const RATE: f32 = 48_000.0;
    // 16 beats at 128 BPM: the loop wraps on a beat
    let take = synth::techno(RATE, 128.0, 7.5, 7);
    if let Ok(mut s) = state.lock() {
        s.name = "Test loop · 128 BPM".into();
        s.hearing = Hearing::Live;
    }
    let (mut analyzer, mut tracker, mut tempo) = (Analyzer::new(RATE), Tracker::new(), Tempo::new(RATE));
    let dt = HOP as f32 / RATE;
    let (start, mut played, mut looked) = (Instant::now(), 0usize, None);
    links.tuning.changed.store(true, Release);
    while !stop.load(Relaxed) {
        if let Some(s) = links.tuning.take() {
            analyzer.set(s);
        }
        look_at_apps(&mut looked, state);
        let due = (start.elapsed().as_secs_f32() * RATE) as usize;
        while played < due {
            let at = played % take.samples.len();
            let n = (due - played).min(take.samples.len() - at);
            links.wave.push_samples(&take.samples[at..at + n]);
            analyzer.push(&take.samples[at..at + n], |hop| hop_out(hop, &mut tracker, dt, &mut tempo, links, state));
            links.wave.push_spectrum(analyzer.spectrum(), analyzer.rate());
            played += n;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    links.features.store([0.0; 12]);
}

/// One hop out to the others: the features, the audio texture's column, the graphs, the tempo.
fn hop_out(hop: &velo_audio::Hop, tracker: &mut Tracker, dt: f32, tempo: &mut Tempo, links: &Links, state: &Mutex<AudioState>) {
    tracker.hop(hop, dt);
    let f = tracker.features();
    links.wave.push_column([f.level, f.bass, f.mid, f.high]);
    links.features.store(f.globals());
    links.meter.record(hop);
    hear_tempo(tempo, hop, state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cards_picture_is_kept_under_all_its_scene_is() {
        let dir = Path::new("shots");
        let node = |id: &str, kind: &str| velo_core::Node {
            id: id.into(),
            kind: kind.into(),
            name: String::new(),
            off: false,
            pos: [0.0; 2],
            params: Default::default(),
            mappings: Vec::new(),
            code: None,
            file: None,
            media: None,
            text: None,
            icon: String::new(),
        };
        let scene = Scene { id: "s_a".into(), nodes: vec![node("n_a", "velo.noise"), node("n_b", "velo.output")], ..Default::default() };
        let file = |s: &Scene| shot_file(dir, s, [1920, 1080]).unwrap();
        let kept = file(&scene);
        assert_eq!(kept, file(&scene.clone()), "the same scene, the same file");
        assert!(kept.starts_with(dir) && kept.extension().is_some_and(|e| e == "png"));
        // a value, a mapping, a wire, a shader's code, the canvas: each is another picture
        let mut changed = vec![scene.clone(); 4];
        changed[0].nodes[0].params.insert("scale".into(), [3.0, 0.0, 0.0, 0.0]);
        changed[1].nodes[0].mappings.push(velo_core::Mapping { param: "scale".into(), source: "kick".into(), amount: 0.5, attack_ms: 5.0, release_ms: 150.0, invert: false });
        changed[2].edges.push(velo_core::Edge { from: ("n_a".into(), 0), to: ("n_b".into(), 0) });
        changed[3].nodes[0].code = Some("fn main() {}".into());
        let mut files: Vec<PathBuf> = changed.iter().map(file).chain([kept, shot_file(dir, &scene, [1280, 720]).unwrap()]).collect();
        files.sort();
        files.dedup();
        assert_eq!(files.len(), 6, "{files:?}");
    }

    #[test]
    fn kept_pictures_come_back_and_the_least_recently_used_go() {
        use velo_engine::runtime::THUMB;
        let dir = std::env::temp_dir().join(format!("velo-shots-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = |name: &str| dir.join(format!("{name}.png"));
        let picture = |shade: u8| vec![shade; (THUMB[0] * THUMB[1] * 4) as usize];
        // used a while ago, each a little later than the one before
        let aged = |name: &str, seconds: u64| std::fs::File::options().write(true).open(file(name)).unwrap().set_modified(SystemTime::now() - Duration::from_secs(seconds)).unwrap();
        assert!(load_shot(&file("a")).is_none(), "nothing kept yet");
        keep_shot(&file("a"), &picture(10), 2);
        aged("a", 300);
        keep_shot(&file("b"), &picture(20), 2);
        aged("b", 200);
        // read: the picture as it was kept, and now the last used
        let a = load_shot(&file("a")).unwrap();
        assert_eq!((a.size, a.pixels[0].to_array()), ([THUMB[0] as usize, THUMB[1] as usize], [10; 4]));
        // a third is one too many: the one not used for longest goes
        keep_shot(&file("c"), &picture(30), 2);
        assert_eq!([file("a").exists(), file("b").exists(), file("c").exists()], [true, false, true]);
        // what isn't a card's picture is no picture, and nothing else in the folder is touched
        std::fs::write(file("d"), b"not a png").unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();
        assert!(load_shot(&file("d")).is_none());
        keep_shot(&file("e"), &picture(40), 1);
        assert!(file("e").exists() && dir.join("notes.txt").exists() && !file("a").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_audio_in_starts_with_the_analyzers_defaults() {
        let params = velo_engine::signal::params(velo_core::AUDIO).unwrap();
        let mut node = velo_core::Node {
            id: "n_a".into(),
            kind: velo_core::AUDIO.into(),
            name: "Audio In".into(),
            off: false,
            pos: [0.0; 2],
            params: Default::default(),
            mappings: Vec::new(),
            code: None,
            file: None,
            media: None,
            text: None,
            icon: String::new(),
        };
        assert_eq!(detection(Some(&node)), Settings::default());
        // its parameters at their defaults read the same as none set
        node.params = params.iter().map(|p| (p.id.clone(), p.default)).collect();
        assert_eq!(detection(Some(&node)), Settings::default());
        // and every one of them lands
        node.params.insert("mode".into(), [0.0, 0.0, 0.0, 0.0]);
        node.params.insert("hat_gap_ms".into(), [120.0, 0.0, 0.0, 0.0]);
        node.params.insert("snare_confidence".into(), [0.7, 0.0, 0.0, 0.0]);
        let s = detection(Some(&node));
        assert_eq!((s.mode, s.detect[2].gap_s, s.model[1]), (Mode::Simple, 0.12, 0.7));
        // through the atomics as they were
        let tuning = Tuning::default();
        tuning.store(&s);
        assert_eq!(tuning.take(), Some(s));
        assert_eq!(tuning.take(), None, "taken once");
    }

    #[test]
    fn every_builtin_has_its_controls() {
        assert_eq!(SHADERS.len(), BUILTINS.len(), "every built-in parses");
        let kaleido = shader("velo.kaleido", None).unwrap();
        let names: Vec<&str> = kaleido.controls.iter().flat_map(|(_, infos)| infos.iter().map(|i| i.name)).collect();
        assert_eq!(names, ["Segments", "Spin", "Zoom", "Angle", "Center x", "Center y", "Automagic", "Amount"], "effects show their automatic amount, last");
        let feedback = shader("velo.feedback", None).unwrap();
        let drift = feedback.controls.iter().find(|(p, _)| p.id == "drift").unwrap();
        assert_eq!(drift.1.iter().map(|i| i.name).collect::<Vec<_>>(), ["Drift x", "Drift y"]);
        assert_eq!(shader("velo.kaleido", None).unwrap().controls[0].1[0].curve, Curve::Steps, "an int moves in steps");
        assert_eq!(label("color_a"), "Color a");
    }

    #[test]
    fn a_shader_node_shows_the_params_of_its_header_as_it_is() {
        let own = shader(velo_core::SHADER, Some(velo_engine::TEMPLATE)).unwrap();
        let names: Vec<&str> = own.controls.iter().flat_map(|(_, infos)| infos.iter().map(|i| i.name)).collect();
        assert_eq!(names, ["Speed", "Rings", "Tint", "Automagic", "Opacity"], "a picture gets an opacity; audio is Automagic");
        // the body changing keeps the same controls; a header that doesn't parse has none
        let edited = velo_engine::TEMPLATE.replace("(1.0 + audio * g.kick)", "(1.0 + 2.0 * audio * g.kick)");
        assert_ne!(edited, velo_engine::TEMPLATE);
        assert!(std::ptr::eq(own, shader(velo_core::SHADER, Some(&edited)).unwrap()));
        assert!(shader(velo_core::SHADER, Some("/*{ \"name\": ")).is_none());
        assert!(shader("velo.noise", Some("anything")).is_some_and(|s| !std::ptr::eq(s, own)), "a built-in ignores code");
    }

    #[test]
    fn the_warm_cache_keeps_what_fits_in_half_the_budget() {
        let held = |sizes: &[(u64, u64)]| sizes.iter().map(|&(key, bytes)| (key, true, bytes)).collect::<Vec<_>>();
        // most recent first: 300 MB in use, then what fits in 1 GB
        let mb = 1 << 20;
        let all = held(&[(1, 300 * mb), (2, 400 * mb), (3, 400 * mb), (4, 100 * mb)]);
        assert_eq!(evictions(&all, &[1], Room::Bytes(1024 * mb)), [3], "what doesn't fit goes; an older, smaller one still fits");
        assert_eq!(evictions(&all, &[1], Room::Bytes(600 * mb)), [3, 2], "the small old one still fits");
        assert!(evictions(&all, &[1, 2, 3, 4], Room::Bytes(mb)).is_empty(), "never one in use, whatever it takes");
        let many: Vec<_> = (1..=20).map(|k| (k, true, mb)).collect();
        assert_eq!(evictions(&many, &[], Room::Bytes(1 << 40)).len(), 20 - WARM_MOST, "a big card still keeps a dozen");
    }

    #[test]
    fn the_warm_cache_keeps_the_last_four_used_and_never_one_in_use() {
        let held = |keys: &[u64]| keys.iter().map(|&k| (k, true, 0)).collect::<Vec<_>>();
        assert!(evictions(&held(&[1, 2, 3, 4]), &[], Room::Graphs(WARM)).is_empty(), "four stay warm");
        // most recently used first: the fifth and sixth go, oldest first
        assert_eq!(evictions(&held(&[6, 5, 4, 3, 2, 1]), &[], Room::Graphs(WARM)), [1, 2]);
        // the live scene, one fading out, a launch and the edited one stay, even beyond four
        assert_eq!(evictions(&held(&[6, 5, 4, 3, 2, 1]), &[1], Room::Graphs(WARM)), [2, 3]);
        assert!(evictions(&held(&[5, 4, 3, 2, 1]), &[1, 2, 3, 4, 5], Room::Graphs(WARM)).is_empty());
        // a deleted scene's graph goes at once, unless it's still out (deleting never blacks out)
        assert_eq!(evictions(&[(1, true, 0), (2, false, 0), (3, false, 0)], &[3], Room::Graphs(WARM)), [2]);
        // a big canvas keeps fewer
        assert_eq!(evictions(&held(&[3, 2, 1]), &[3], Room::Graphs(1)), [1, 2]);
        // launching back and forth between two keeps both: each launch makes it the most recent
        let mut order = vec![1, 2, 3, 4];
        for launch in [5, 1, 5, 1] {
            order.retain(|&k| k != launch);
            order.insert(0, launch);
            let gone = evictions(&held(&order), &[launch], Room::Graphs(WARM));
            order.retain(|k| !gone.contains(k));
        }
        assert_eq!(order, [1, 5, 2, 3], "the least recently used (4) went");
    }

    #[test]
    fn only_structure_rebuilds() {
        let mut s = velo_core::Session::default();
        s.run("scene.add", serde_json::json!({})).unwrap();
        s.run("node.add", serde_json::json!({ "kind": "velo.noise" })).unwrap();
        let scene = s.active().unwrap().clone();
        let base = structure(Some(&scene), [1920, 1080]);
        let mut moved = scene.clone();
        moved.nodes[0].pos = [100.0, 0.0];
        moved.nodes[0].name = "Fog".into();
        moved.nodes[0].params.insert("scale".into(), [5.0; 4]);
        moved.nodes[0].mappings.push(velo_core::Mapping { param: "scale".into(), source: "kick".into(), amount: 0.5, attack_ms: 5.0, release_ms: 150.0, invert: false });
        assert_eq!(structure(Some(&moved), [1920, 1080]), base, "moving, renaming, values and mappings don't rebuild");
        assert_ne!(structure(Some(&scene), [1280, 720]), base, "the canvas size does");
        let mut off = scene.clone();
        off.nodes[0].off = true;
        assert_ne!(structure(Some(&off), [1920, 1080]), base, "turning a node off rebuilds");
        let mut other = scene.clone();
        other.nodes[0].kind = "velo.gradient".into();
        assert_ne!(structure(Some(&other), [1920, 1080]), base, "a node's kind does");
    }

    /// Not a check: `VELO_PROFILE=<project> cargo test -p velo scene_costs -- --ignored --nocapture`
    /// builds each scene of a project at its canvas size and prints what a frame of it costs the
    /// GPU, its costliest nodes, and what the nodes nothing goes out through cost.
    #[test]
    #[ignore]
    fn scene_costs() {
        use eframe::wgpu;
        let path = std::env::var_os("VELO_PROFILE").expect("VELO_PROFILE=<project>");
        let (project, _) = crate::files::read(std::path::Path::new(&path)).unwrap();
        let engine = velo_engine::Engine::start(None).unwrap();
        let (device, queue) = (engine.device(), engine.queue());
        assert!(device.features().contains(wgpu::Features::TIMESTAMP_QUERY), "no timestamps on this GPU");
        let compiler = Compiler::new(device.clone(), queue.clone(), engine.wave_texture().clone());
        let size = [project.canvas.width, project.canvas.height];
        let n = velo_engine::gpu::MAX_TIMED as u64 * 2;
        let set = device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: n as u32 });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n * 8, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let back = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: n * 8, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut total = 0.0;
        for (k, scene) in project.scenes.iter().enumerate() {
            // twice: the first build starts the text and image loads, the second has them
            compiler.build(k as u64, scene.clone(), size);
            let _ = compiler.built.recv();
            std::thread::sleep(Duration::from_millis(300));
            compiler.build(k as u64, scene.clone(), size);
            let Ok((_, Ok(mut built))) = compiler.built.recv() else { continue };
            let g = &mut built.graph;
            // what renders as the engine runs it: what a Switch or a clear Blend doesn't show sleeps
            let awake = g.output.map(|out| g.runner.needed([out], |n, i| g.lazy.iter().find(|l| l.0 == n).is_none_or(|(_, l)| l.reads(|p| g.mods[n].value(p), i))));
            // each node's best of 30 frames, after 30 to warm up
            let mut best = vec![f32::MAX; g.runner.len().min(velo_engine::gpu::MAX_TIMED)];
            for f in 0..60 {
                let timed = f >= 30;
                let mut encoder = device.create_command_encoder(&Default::default());
                let globals = [f as f32 / 60.0, 1.0 / 60.0, f as f32, size[0] as f32 / size[1] as f32, 120.0, 0.0, 0.0];
                g.runner.render_timed(queue, &mut encoder, &g.programs, &globals, timed.then_some(&set), awake.as_deref());
                if timed {
                    encoder.resolve_query_set(&set, 0..n as u32, &resolve, 0);
                    encoder.copy_buffer_to_buffer(&resolve, 0, &back, 0, n * 8);
                }
                queue.submit([encoder.finish()]);
                if timed {
                    back.map_async(wgpu::MapMode::Read, .., |r| r.expect("map"));
                    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                    let ticks: Vec<u64> = back.slice(..).get_mapped_range().unwrap().as_chunks::<8>().0.iter().map(|b| u64::from_le_bytes(*b)).collect();
                    back.unmap();
                    for (i, b) in best.iter_mut().enumerate() {
                        let asleep = awake.as_ref().is_some_and(|a| !a[i]);
                        *b = if asleep { 0.0 } else { b.min(ticks[2 * i + 1].saturating_sub(ticks[2 * i]) as f32 * queue.get_timestamp_period() / 1e6) };
                    }
                }
            }
            // what reaches the output, walking up from it
            let mut used = vec![false; built.plan.nodes.len()];
            let mut todo: Vec<usize> = built.plan.output.into_iter().collect();
            while let Some(i) = todo.pop() {
                if !std::mem::replace(&mut used[i], true) {
                    todo.extend(built.plan.inputs[i].iter().flatten());
                }
            }
            let name = |i: usize| scene.node(&built.plan.nodes[i].0).map_or_else(String::new, |n| format!("{} ({})", n.name, n.kind));
            let sum: f32 = best.iter().sum();
            let unused: f32 = best.iter().zip(&used).filter(|(_, u)| !**u).map(|(b, _)| b).sum();
            total += sum;
            let mut top: Vec<usize> = (0..best.len()).collect();
            top.sort_by(|a, b| best[*b].total_cmp(&best[*a]));
            let top: Vec<String> = top.iter().take(3).map(|&i| format!("{} {:.2}", name(i), best[i])).collect();
            let asleep = awake.as_ref().map_or(0, |a| a.iter().filter(|a| !**a).count());
            println!("{sum:5.2} ms  {:2} nodes ({asleep} asleep), {unused:.2} ms in {} unused  {}: {}", used.len(), used.iter().filter(|u| !**u).count(), scene.name, top.join(", "));
        }
        println!("{total:.2} ms for all {} scenes at {}×{}", project.scenes.len(), size[0], size[1]);
    }
}
