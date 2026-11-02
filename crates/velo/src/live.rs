//! What runs live behind the UI: the engine (scene graphs built for it, launches, its preview
//! back as an egui texture) and the audio input. The UI thread never waits on either: graphs are
//! built on the compiler's thread, audio is opened and analysed on its own thread, and
//! everything comes back through queues and atomics.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

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
    fn of(params: &'static [Param]) -> Self {
        Self { controls: params.iter().map(|p| (p, infos(p))).collect() }
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
/// instant. More stay only while in use (live, fading out, launched, edited).
pub const WARM: usize = 4;

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
}

pub struct Live {
    pub engine: Rc<Engine>,
    pub shared: Rc<RefCell<Shared>>,
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
                    self.errors.retain(|id, _| !b.plan.nodes.iter().any(|(n, _)| n == id));
                    self.errors.extend(b.errors);
                    slot.installed = b.plan.nodes.into_iter().map(|(id, _)| id).zip(b.shaders).collect();
                    (slot.signals, slot.kinds) = (b.signals, None);
                    slot.media = b
                        .media
                        .into_iter()
                        .map(|(id, l)| {
                            let built = l.get().is_some();
                            (id, l, built)
                        })
                        .collect();
                    // the build set the values it was asked with; anything newer goes below
                    slot.sent = vec![Vec::new(); slot.installed.len()];
                    slot.routed = vec![Vec::new(); slot.installed.len()];
                    (slot.macros, slot.driving) = (None, None);
                    self.engine.send(Command::Install { key, graph: b.graph });
                }
                Err(e) => error = Some(e),
            }
        }
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
                let values = shader.values(|p| node.params.get(p).copied());
                if slot.sent[n] != values {
                    self.engine.send(Command::Params { key: slot.key, node: n, values: values.clone() });
                    slot.sent[n] = values;
                }
                let routes = routes(shader, &node.mappings, &slot.signals);
                if slot.routed[n] != routes {
                    self.engine.send(Command::Mappings { key: slot.key, node: n, routes: routes.clone() });
                    slot.routed[n] = routes;
                }
            }
        }
        let status = &self.engine.status;
        let (live, fading) = (status.live.load(Relaxed), status.fading.load(Relaxed));
        let in_use = [live, fading, status.armed().map_or(0, |a| a.0), self.launched, self.edit];
        let held: Vec<(u64, bool)> = self.slots.iter().map(|s| (s.key, project.scene(&s.scene).is_some())).collect();
        // fewer scenes kept warm as the canvas grows: four at 1080p, one at 4K
        let warm = (WARM as u64 * 1920 * 1080 / (u64::from(size[0]) * u64::from(size[1])).max(1)).clamp(1, WARM as u64) as usize;
        for key in evictions(&held, &in_use, warm) {
            self.slots.retain(|s| s.key != key);
            self.engine.send(Command::Evict(key));
        }
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
                let image = egui::ColorImage::from_rgba_premultiplied([cell_w, cell_h], &cell);
                match self.shots.get_mut(&scene) {
                    Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        let texture = ctx.load_texture(format!("shot {scene}"), image, egui::TextureOptions::LINEAR);
                        self.shots.insert(scene, texture);
                    }
                }
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

    pub fn launch(&mut self, scene: &Scene) {
        self.launch_with(scene, scene.launch);
    }

    fn launch_with(&mut self, scene: &Scene, launch: Launch) {
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

    /// A node of the edited scene, with its values as the engine modulated them last frame (x
    /// and y per parameter, in header order), while it's mapped; asking makes the engine
    /// publish them.
    pub fn modulated(&self, node: &str) -> Option<[f32; 32]> {
        let slot = self.slots.iter().find(|s| s.key == self.edit)?;
        let n = slot.installed.iter().position(|(id, _)| id == node)?;
        self.engine.status.watch(n)
    }

    /// What a signal node's output reads now in the edited scene's graph (its card's meter).
    pub fn signal(&self, node: &str, port: u8) -> Option<f32> {
        let slot = self.slots.iter().find(|s| s.key == self.edit)?;
        let at = slot.signals.iter().position(|k| *k == format!("{node}/{port}"))?;
        Some(self.engine.status.signal(at))
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

/// The warm cache's evictions: given the held graphs most recently used first (each with
/// whether its scene still exists), the ones to drop. A deleted scene's goes at once, the least
/// recently used go beyond `warm` (at most [`WARM`]); never one in use.
fn evictions(held: &[(u64, bool)], in_use: &[u64], warm: usize) -> Vec<u64> {
    let mut kept = held.len();
    let mut out = Vec::new();
    for &(key, exists) in held.iter().rev() {
        if !in_use.contains(&key) && (!exists || kept > warm) {
            out.push(key);
            kept -= 1;
        }
    }
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
}

impl Meter {
    fn record(&self, hop: &velo_audio::Hop) {
        self.level.fetch_max((hop.level_db + 200.0).max(0.0).to_bits(), Relaxed);
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
    fn the_warm_cache_keeps_the_last_four_used_and_never_one_in_use() {
        let held = |keys: &[u64]| keys.iter().map(|&k| (k, true)).collect::<Vec<_>>();
        assert!(evictions(&held(&[1, 2, 3, 4]), &[], WARM).is_empty(), "four stay warm");
        // most recently used first: the fifth and sixth go, oldest first
        assert_eq!(evictions(&held(&[6, 5, 4, 3, 2, 1]), &[], WARM), [1, 2]);
        // the live scene, one fading out, a launch and the edited one stay, even beyond four
        assert_eq!(evictions(&held(&[6, 5, 4, 3, 2, 1]), &[1], WARM), [2, 3]);
        assert!(evictions(&held(&[5, 4, 3, 2, 1]), &[1, 2, 3, 4, 5], WARM).is_empty());
        // a deleted scene's graph goes at once, unless it's still out (deleting never blacks out)
        assert_eq!(evictions(&[(1, true), (2, false), (3, false)], &[3], WARM), [2]);
        // a big canvas keeps fewer
        assert_eq!(evictions(&held(&[3, 2, 1]), &[3], 1), [1, 2]);
        // launching back and forth between two keeps both: each launch makes it the most recent
        let mut order = vec![1, 2, 3, 4];
        for launch in [5, 1, 5, 1] {
            order.retain(|&k| k != launch);
            order.insert(0, launch);
            let gone = evictions(&held(&order), &[launch], WARM);
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
}
