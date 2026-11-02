//! Sources → mappings → parameter values (docs/graph.md §3, §5 steps 3 and 5), run on the
//! render thread every frame: the beat clock, the scene's macros (docs/scenes.md §2: a driven
//! parameter's base is its range at the macro's value, morphing to a variation on the beat
//! clock), and each mapped node's values recomputed from its base values, its mappings'
//! smoothed sources and its shader's ranges. The UI never computes them; it sends base values,
//! macro values and [`Route`]s and reads the results back for display.
//! Nothing here allocates per frame.

use velo_core::{MACROS, Macro, Mapping, SOURCES};

use crate::shader::{ParamType, ShaderNode};

/// A mapping as the engine runs it: indices instead of names, smoothing in seconds, and the
/// smoothed source it carries from frame to frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Route {
    /// The parameter, by its index in the shader header, and which of its vec4's floats.
    pub param: usize,
    pub axis: usize,
    /// An index into [`SOURCES`].
    pub source: usize,
    /// -1..1 of the parameter's range.
    pub amount: f32,
    pub attack: f32,
    pub release: f32,
    pub invert: bool,
    /// The source after smoothing: engine state, 0 when the UI sends a route.
    level: f32,
}

/// A parameter by its mapping name ("zoom", a vec2's axis as "offset.x" / "offset.y"): its
/// index in the shader header and which float of its vec4, if it's one a mapping can move (not
/// on/offs; a color, round its hue).
pub fn slot(node: &ShaderNode, param: &str) -> Option<(usize, usize)> {
    let (id, axis) = match param.rsplit_once('.') {
        Some((id, "y")) => (id, 1),
        Some((id, "x")) => (id, 0),
        _ => (param, 0),
    };
    let index = node.params.iter().position(|p| p.id == id && matches!(p.ty, ParamType::Float | ParamType::Int | ParamType::Vec2 | ParamType::Color))?;
    Some((index, axis))
}

/// A color turned round the grey axis by `turns` (the feedback shader's hue_rotate), in 0..1:
/// a third of a turn takes red to green.
fn hue_turn(c: &mut [f32; 4], turns: f32) {
    let (sin, cos) = (turns * std::f32::consts::TAU).sin_cos();
    let k = 1.0 / 3f32.sqrt();
    let v = [c[0], c[1], c[2]];
    let along = k * k * (v[0] + v[1] + v[2]) * (1.0 - cos);
    let cross = [k * (v[2] - v[1]), k * (v[0] - v[2]), k * (v[1] - v[0])];
    for i in 0..3 {
        c[i] = (v[i] * cos + cross[i] * sin + along).clamp(0.0, 1.0);
    }
}

/// A node's mappings resolved against its shader. A mapping naming a parameter the shader
/// doesn't have (or can't move: on/offs) or an unknown source is left out.
/// Mappings resolved against a shader: the audio and clock sources by name, the graph's
/// `signals` (its LFOs' node ids) after them.
pub fn routes(node: &ShaderNode, mappings: &[Mapping], signals: &[String]) -> Vec<Route> {
    mappings
        .iter()
        .filter_map(|m| {
            let (param, axis) = slot(node, &m.param)?;
            // a shared source by name, or a signal node's output ("n_ab12/0"; an LFO by its id alone)
            let key = match velo_core::signal_output(&m.source) {
                Some((id, port)) => format!("{id}/{port}"),
                None => m.source.clone(),
            };
            let source = SOURCES.iter().position(|s| *s == m.source).or_else(|| signals.iter().position(|s| *s == key).map(|i| SOURCES.len() + i))?;
            Some(Route { param, axis, source, amount: m.amount, attack: m.attack_ms / 1000.0, release: m.release_ms / 1000.0, invert: m.invert, level: 0.0 })
        })
        .collect()
}

/// [`slot`], but only a number's: a macro's range doesn't mean anything for a color.
fn number_slot(node: &ShaderNode, param: &str) -> Option<(usize, usize)> {
    slot(node, param).filter(|&(i, _)| node.params[i].ty != ParamType::Color)
}

/// A macro target as the engine runs it: which macro, the engine's node, the parameter's slot
/// and the values at the macro's ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MacroRoute {
    pub index: usize,
    pub node: usize,
    pub param: usize,
    pub axis: usize,
    pub min: f32,
    pub max: f32,
}

/// A scene's macro targets resolved against its graph: `nodes` are the engine's nodes in order,
/// each with its scene node's id and its shader. Targets on nodes or parameters the graph
/// doesn't run are left out.
pub fn macro_routes(nodes: &[(&str, &ShaderNode)], macros: &[Macro]) -> Vec<MacroRoute> {
    let mut out = Vec::new();
    for (index, m) in macros.iter().enumerate() {
        for t in &m.targets {
            let Some(node) = nodes.iter().position(|n| n.0 == t.node) else { continue };
            if let Some((param, axis)) = number_slot(nodes[node].1, &t.param) {
                out.push(MacroRoute { index, node, param, axis, min: t.min, max: t.max });
            }
        }
    }
    out
}

/// A graph's eight macros on the render thread: their values, a morph to a variation under
/// way, and what they drive. Values are set one at a time from the UI (a knob, MIDI); a morph
/// glides all eight from where they are to new values over some beats of the clock. Setting a
/// macro during a morph takes that one out of it.
#[derive(Clone, Debug, Default)]
pub struct Macros {
    from: [f32; MACROS],
    to: [f32; MACROS],
    /// The morph: the clock's beat it started on and how many beats it takes.
    morph: Option<(f64, f64)>,
    pub routes: Vec<MacroRoute>,
}

impl Macros {
    pub fn new(values: [f32; MACROS], routes: Vec<MacroRoute>) -> Self {
        Self { from: values, to: values, morph: None, routes }
    }

    /// Macro `index` at `value`, at once.
    pub fn set(&mut self, index: usize, value: f32) {
        if index < MACROS {
            self.from[index] = value;
            self.to[index] = value;
        }
    }

    /// From where they are at `beats` on the clock to `to`, over `over` beats (0: at once).
    pub fn morph(&mut self, beats: f64, to: [f32; MACROS], over: f32) {
        self.from = self.values(beats);
        self.to = to;
        self.morph = (over > 0.0).then_some((beats, f64::from(over)));
    }

    /// Where they are at `beats` on the clock. A clock that jumps back (a resync) holds the
    /// morph at its start; one that jumps ahead finishes it.
    pub fn values(&self, beats: f64) -> [f32; MACROS] {
        let k = self.morph.map_or(1.0, |(start, over)| ((beats - start) / over).clamp(0.0, 1.0) as f32);
        std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * k)
    }

    /// Writes each driven parameter's base: its range at its macro's value, inside the
    /// shader's range. Mappings add on top in [`Modulator::step`].
    pub fn apply(&self, beats: f64, mods: &mut [Modulator]) {
        if self.routes.is_empty() {
            return;
        }
        let values = self.values(beats);
        for r in &self.routes {
            if let Some(m) = mods.get_mut(r.node) {
                m.set_base_at(r.param, r.axis, r.min + (r.max - r.min) * values[r.index].clamp(0.0, 1.0));
            }
        }
    }

    /// Marks the nodes the macros drive: their values are written every frame.
    pub fn drive(&self, mods: &mut [Modulator]) {
        for (n, m) in mods.iter_mut().enumerate() {
            m.driven = self.routes.iter().any(|r| r.node == n);
        }
    }
}

/// One node's parameter values on the render thread: what the UI set, the shader's ranges,
/// the routes, and room for the result (allocated once, when the graph is built).
pub struct Modulator {
    base: Vec<[f32; 4]>,
    ranges: Vec<[f32; 2]>,
    /// Which parameters are colors: a mapping turns their hue instead of adding to them.
    colors: Vec<bool>,
    routes: Vec<Route>,
    /// A macro drives one of its parameters: written every frame, like a mapped node.
    pub driven: bool,
    out: Vec<[f32; 4]>,
    bytes: Vec<u8>,
}

impl Modulator {
    pub fn new(node: &ShaderNode, base: &[[f32; 4]], routes: Vec<Route>) -> Self {
        let n = node.params.len();
        let mut m = Self {
            base: node.params.iter().map(|p| p.default).collect(),
            ranges: node.params.iter().map(|p| [p.min, p.max]).collect(),
            colors: node.params.iter().map(|p| p.ty == ParamType::Color).collect(),
            routes,
            driven: false,
            out: vec![[0.0; 4]; n],
            // the buffer is at least one vec4 (an empty uniform array isn't allowed)
            bytes: vec![0; n.max(1) * 16],
        };
        m.set_base(base);
        m
    }

    /// The values as the UI set them (copied in place: the render thread doesn't allocate).
    pub fn set_base(&mut self, values: &[[f32; 4]]) {
        for (b, v) in self.base.iter_mut().zip(values) {
            *b = *v;
        }
    }

    /// One float of one parameter's base, inside the shader's range (a macro's doing).
    pub fn set_base_at(&mut self, param: usize, axis: usize, value: f32) {
        if let (Some(b), Some(&[min, max])) = (self.base.get_mut(param), self.ranges.get(param)) {
            b[axis.min(3)] = value.max(min.min(max)).min(max.max(min));
        }
    }

    /// New routes; the ones that stay (same parameter, axis and source) keep their smoothed
    /// level, so dragging an amount doesn't restart the motion.
    pub fn set_routes(&mut self, mut routes: Vec<Route>) {
        for r in &mut routes {
            if let Some(old) = self.routes.iter().find(|o| (o.param, o.axis, o.source) == (r.param, r.axis, r.source)) {
                r.level = old.level;
            }
        }
        self.routes = routes;
    }

    /// Mapped or driven by a macro: its values move every frame.
    pub fn active(&self) -> bool {
        self.driven || !self.routes.is_empty()
    }

    /// One frame, `dt` seconds after the last: smooths each route's source (one-pole, attack
    /// when it rises, release when it falls), adds `amount` × range × level to the base,
    /// clamps what moved to the range; a color turns round its hue, `amount` × level turns.
    /// Returns the parameter buffer's bytes.
    pub fn step(&mut self, sources: &[f32], dt: f32) -> &[u8] {
        self.out.copy_from_slice(&self.base);
        for r in &mut self.routes {
            let x = sources.get(r.source).copied().unwrap_or(0.0).clamp(0.0, 1.0);
            let x = if r.invert { 1.0 - x } else { x };
            let tau = if x > r.level { r.attack } else { r.release };
            r.level += (x - r.level) * if tau > 0.0 { 1.0 - (-dt / tau).exp() } else { 1.0 };
            if self.colors[r.param] {
                hue_turn(&mut self.out[r.param], r.amount * r.level);
                continue;
            }
            let [min, max] = self.ranges[r.param];
            self.out[r.param][r.axis] += r.amount * (max - min) * r.level;
        }
        for r in self.routes.iter().filter(|r| !self.colors[r.param]) {
            let [min, max] = self.ranges[r.param];
            // max/min, not clamp: a user shader's header may have them the wrong way round
            let v = &mut self.out[r.param][r.axis];
            *v = v.max(min).min(max);
        }
        fill(&mut self.bytes, &self.out)
    }

    /// The base values as the parameter buffer's bytes: what goes back in once nothing is mapped.
    pub fn base_bytes(&mut self) -> &[u8] {
        fill(&mut self.bytes, &self.base)
    }

    /// A parameter's value now (its first axis): as the last step left it while it moves,
    /// else as set.
    pub fn value(&self, param: usize) -> f32 {
        let values = if self.active() { &self.out } else { &self.base };
        values.get(param).map_or(0.0, |v| v[0])
    }

    /// The values the last [`Self::step`] computed.
    pub fn values(&self) -> &[[f32; 4]] {
        &self.out
    }
}

fn fill<'a>(bytes: &'a mut [u8], values: &[[f32; 4]]) -> &'a [u8] {
    for (chunk, v) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values.iter().flatten()) {
        *chunk = v.to_ne_bytes();
    }
    bytes
}

// ---------------------------------------------------------------------------------------------
// The beat clock

/// Taps averaged for the tempo.
pub const TAPS: usize = 8;
/// A pause this long (seconds) starts a new series of taps.
const TAP_RESET: f64 = 2.0;
pub const MIN_BPM: f32 = 20.0;
pub const MAX_BPM: f32 = 300.0;
/// How long the beat's hit takes to fall to 1/e, as the audio hits do.
const BEAT_DECAY: f32 = 0.15;

/// The beat clock (docs/control.md §4): a tempo and when beat 0 (a downbeat) fell, so the
/// phase is a function of time and never drifts with frame timing. Tap tempo and resync move
/// them. Times are seconds on the render thread's clock.
#[derive(Clone, Debug)]
pub struct Clock {
    bpm: f32,
    origin: f64,
    /// The current series' last taps, oldest first, and how many taps the series has had.
    taps: [f64; TAPS],
    held: usize,
    series: u64,
}

impl Default for Clock {
    fn default() -> Self {
        Self { bpm: 120.0, origin: 0.0, taps: [0.0; TAPS], held: 0, series: 0 }
    }
}

impl Clock {
    pub fn bpm(&self) -> f32 {
        self.bpm
    }

    /// Beats since beat 0 at time `t` (what launches quantize on).
    pub fn beats(&self, t: f64) -> f64 {
        (t - self.origin) * f64::from(self.bpm) / 60.0
    }

    /// At time `t`: the ramp through the beat, the ramp through the 4/4 bar, and the beat's
    /// hit (1 on the beat, falling back).
    pub fn at(&self, t: f64) -> [f32; 3] {
        let beats = self.beats(t);
        let beat_phase = beats.rem_euclid(1.0) as f32;
        let since = beat_phase * 60.0 / self.bpm;
        [beat_phase, (beats / 4.0).rem_euclid(1.0) as f32, (-since / BEAT_DECAY).exp()]
    }

    /// A new tempo from `t` on; the phase carries on without a jump.
    pub fn set_bpm(&mut self, t: f64, bpm: f32) {
        if !bpm.is_finite() {
            return;
        }
        let beats = self.beats(t);
        self.bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        self.origin = t - beats * 60.0 / f64::from(self.bpm);
    }

    /// A downbeat at `t`.
    pub fn resync(&mut self, t: f64) {
        self.origin = t;
    }

    /// An outside clock's beat at `t`: its tempo, and where it is (beats from its downbeat,
    /// a fraction for Link's), or, when that isn't known, the nearest beat: the phase locks to it.
    pub fn follow(&mut self, t: f64, bpm: f32, beat: Option<f64>) {
        if !bpm.is_finite() {
            return;
        }
        let at = beat.filter(|b| b.is_finite()).unwrap_or_else(|| self.beats(t).round());
        self.bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        self.origin = t - at * 60.0 / f64::from(self.bpm);
    }

    /// A tap at `t`. The first of a series (after a 2-s pause) puts a downbeat on it; each
    /// later one is the next beat: the tempo becomes the average gap of the last 8 taps, and
    /// the beat lands on the tap.
    pub fn tap(&mut self, t: f64) {
        let last = self.taps[..self.held].last().copied();
        if last.is_none_or(|last| t - last > TAP_RESET || t <= last) {
            (self.taps[0], self.held, self.series) = (t, 1, 1);
            self.resync(t);
            return;
        }
        if self.held == TAPS {
            self.taps.copy_within(1.., 0);
            self.held -= 1;
        }
        self.taps[self.held] = t;
        self.held += 1;
        self.series += 1;
        let gap = (t - self.taps[0]) / (self.held - 1) as f64;
        // to the half BPM, as tracks are
        self.bpm = (((120.0 / gap) as f32).round() / 2.0).clamp(MIN_BPM, MAX_BPM);
        self.origin = t - (self.series - 1) as f64 * 60.0 / f64::from(self.bpm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BUILTINS;

    fn shader(id: &str) -> ShaderNode {
        ShaderNode::parse(BUILTINS.iter().find(|b| b.0 == id).unwrap().1).unwrap()
    }

    fn mapping(param: &str, source: &str, amount: f32) -> Mapping {
        Mapping { param: param.into(), source: source.into(), amount, attack_ms: 0.0, release_ms: 0.0, invert: false }
    }

    /// Kick is source 4.
    fn kick(level: f32) -> [f32; 9] {
        let mut s = [0.0; 9];
        s[4] = level;
        s
    }

    #[test]
    fn mappings_resolve_to_parameters_axes_and_sources() {
        let feedback = shader("feedback");
        let r = routes(&feedback, &[mapping("zoom", "kick", 0.5), mapping("drift.y", "bar_phase", -1.0), mapping("nope", "kick", 1.0), mapping("zoom", "cowbell", 1.0)], &[]);
        assert_eq!(r.iter().map(|r| (r.param, r.axis, r.source)).collect::<Vec<_>>(), [(1, 0, 4), (3, 1, 8)], "unknown parameters and sources are left out");
        assert!(routes(&shader("color"), &[mapping("invert", "kick", 1.0)], &[]).is_empty(), "on/offs don't take mappings");
    }

    #[test]
    fn the_value_is_base_plus_amount_times_range_clamped() {
        // kaleido zoom: 0.25..4, base 1
        let node = shader("kaleido");
        let zoom = node.params.iter().position(|p| p.id == "zoom").unwrap();
        let base = node.values(|p| (p == "zoom").then_some([1.0, 0.0, 0.0, 0.0]));
        let mut m = Modulator::new(&node, &base, routes(&node, &[mapping("zoom", "kick", 0.2)], &[]));
        m.step(&kick(0.0), 0.016);
        assert_eq!(m.values()[zoom][0], 1.0, "silence: the base");
        m.step(&kick(1.0), 0.016);
        assert!((m.values()[zoom][0] - (1.0 + 0.2 * 3.75)).abs() < 1e-5, "{}", m.values()[zoom][0]);
        // two sources add up, and the sum stops at the range's end
        m.set_routes(routes(&node, &[mapping("zoom", "kick", 0.9), mapping("zoom", "level", 0.9)], &[]));
        let mut both = kick(1.0);
        both[0] = 1.0;
        m.step(&both, 0.016);
        assert_eq!(m.values()[zoom][0], 4.0);
        m.set_routes(routes(&node, &[mapping("zoom", "kick", -1.0)], &[]));
        m.step(&kick(1.0), 0.016);
        assert_eq!(m.values()[zoom][0], 0.25, "a negative amount pulls down, to the minimum");
        // the bytes are the values, and only mapped floats move
        let bytes = m.step(&kick(0.5), 0.016).to_vec();
        assert_eq!(bytes.len(), node.params.len() * 16);
        assert_eq!(f32::from_ne_bytes(bytes[zoom * 16..zoom * 16 + 4].try_into().unwrap()), m.values()[zoom][0]);
        assert_eq!(m.values()[0], base[0], "segments isn't mapped");
        // inverted: full in silence
        m.set_routes(routes(&node, &[Mapping { invert: true, ..mapping("zoom", "kick", 0.2) }], &[]));
        m.step(&kick(0.0), 0.016);
        assert!((m.values()[zoom][0] - 1.75).abs() < 1e-5);
        m.set_routes(Vec::new());
        assert!(!m.active());
        assert_eq!(f32::from_ne_bytes(m.base_bytes()[zoom * 16..zoom * 16 + 4].try_into().unwrap()), 1.0, "unmapped, it goes back to the base");
    }

    #[test]
    fn a_mapped_color_turns_round_its_hue() {
        // shapes' tint, red: a third of a turn at full kick makes it green, then back
        let node = shader("shapes");
        let tint = node.params.iter().position(|p| p.id == "tint").unwrap();
        let base = node.values(|p| (p == "tint").then_some([1.0, 0.0, 0.0, 1.0]));
        let mut m = Modulator::new(&node, &base, routes(&node, &[mapping("tint", "kick", 1.0 / 3.0)], &[]));
        m.step(&kick(1.0), 0.016);
        let c = m.values()[tint];
        assert!(c[0] < 1e-4 && (c[1] - 1.0).abs() < 1e-4 && c[2] < 1e-4 && c[3] == 1.0, "{c:?}");
        m.step(&kick(0.0), 0.016);
        assert_eq!(m.values()[tint], [1.0, 0.0, 0.0, 1.0]);
        // a macro's range is a number: it never drives a color
        let target = velo_core::MacroTarget { node: "n".into(), param: "tint".into(), min: 0.0, max: 1.0 };
        let macros = [velo_core::Macro { name: String::new(), value: 0.5, targets: vec![target] }];
        assert!(macro_routes(&[("n", &node)], &macros).is_empty());
    }

    #[test]
    fn smoothing_rises_with_attack_and_falls_with_release() {
        let node = shader("kaleido");
        let zoom = node.params.iter().position(|p| p.id == "zoom").unwrap();
        let slow = Mapping { attack_ms: 10.0, release_ms: 100.0, ..mapping("zoom", "kick", 1.0 / 3.75) };
        let mut m = Modulator::new(&node, &node.values(|_| None), routes(&node, std::slice::from_ref(&slow), &[]));
        let level = |m: &Modulator| m.values()[zoom][0] - 1.0;
        // one attack time in: 1 - 1/e of the way up
        for _ in 0..10 {
            m.step(&kick(1.0), 0.001);
        }
        assert!((level(&m) - (1.0 - (-1.0f32).exp())).abs() < 1e-3, "{}", level(&m));
        for _ in 0..200 {
            m.step(&kick(1.0), 0.001);
        }
        assert!((level(&m) - 1.0).abs() < 1e-3);
        // one release time later: 1/e left
        for _ in 0..100 {
            m.step(&kick(0.0), 0.001);
        }
        assert!((level(&m) - (-1.0f32).exp()).abs() < 1e-3, "{}", level(&m));
        // changing the amount keeps the smoothed level: no restart
        let before = level(&m);
        m.set_routes(routes(&node, &[Mapping { amount: 2.0 / 3.75, ..slow }], &[]));
        m.step(&kick(0.0), 0.0);
        assert!((level(&m) - 2.0 * before).abs() < 1e-4);
    }

    fn target(node: &str, param: &str, min: f32, max: f32) -> velo_core::MacroTarget {
        velo_core::MacroTarget { node: node.into(), param: param.into(), min, max }
    }

    #[test]
    fn macro_targets_resolve_to_engine_nodes_and_slots() {
        let (kaleido, feedback) = (shader("kaleido"), shader("feedback"));
        let nodes = [("n_k", &kaleido), ("n_f", &feedback)];
        let mut macros: [Macro; MACROS] = Default::default();
        macros[2].targets = vec![target("n_f", "drift.y", -1.0, 1.0), target("n_k", "zoom", 1.0, 2.0), target("n_gone", "zoom", 0.0, 1.0), target("n_k", "nope", 0.0, 1.0)];
        macros[5].targets = vec![target("n_k", "spin", 0.0, 0.5)];
        let r = macro_routes(&nodes, &macros);
        let zoom = kaleido.params.iter().position(|p| p.id == "zoom").unwrap();
        let spin = kaleido.params.iter().position(|p| p.id == "spin").unwrap();
        assert_eq!(r.iter().map(|r| (r.index, r.node, r.param, r.axis)).collect::<Vec<_>>(), [(2, 1, 3, 1), (2, 0, zoom, 0), (5, 0, spin, 0)], "unknown nodes and parameters are left out");
    }

    #[test]
    fn a_macro_sets_the_base_and_mappings_add_on_top() {
        // kaleido zoom: 0.25..4; macro 0 takes it from 1 to 3, the kick adds 0.2 of the range
        let node = shader("kaleido");
        let zoom = node.params.iter().position(|p| p.id == "zoom").unwrap();
        let mut mods = vec![Modulator::new(&node, &node.values(|_| None), routes(&node, &[mapping("zoom", "kick", 0.2)], &[]))];
        let mut macros: [Macro; MACROS] = Default::default();
        macros[0].targets = vec![target("n", "zoom", 1.0, 3.0)];
        let mut m = Macros::new([0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], macro_routes(&[("n", &node)], &macros));
        m.drive(&mut mods);
        assert!(mods[0].active());
        m.apply(0.0, &mut mods);
        mods[0].step(&kick(0.0), 0.016);
        assert!((mods[0].values()[zoom][0] - 2.0).abs() < 1e-6, "half way from 1 to 3");
        mods[0].step(&kick(1.0), 0.016);
        assert!((mods[0].values()[zoom][0] - (2.0 + 0.2 * 3.75)).abs() < 1e-5, "the kick on top");
        // the macro's ends stay inside the shader's range
        m.set(0, 1.0);
        m.routes[0].max = 9.0;
        m.apply(0.0, &mut mods);
        mods[0].step(&kick(0.0), 0.016);
        assert_eq!(mods[0].values()[zoom][0], 4.0);
        // no targets: nothing driven
        m.routes.clear();
        m.drive(&mut mods);
        assert!(mods[0].active(), "still mapped");
        mods[0].set_routes(Vec::new());
        assert!(!mods[0].active());
    }

    #[test]
    fn a_morph_glides_on_the_beat_clock_and_a_set_takes_one_macro_out() {
        let mut m = Macros::new([0.0; MACROS], Vec::new());
        let mut to = [0.0; MACROS];
        to[0] = 1.0;
        to[1] = 0.5;
        // from beat 10 over 4 beats
        m.morph(10.0, to, 4.0);
        assert_eq!(m.values(10.0)[0], 0.0);
        assert_eq!(m.values(11.0)[..2], [0.25, 0.125]);
        assert_eq!(m.values(14.0)[..2], [1.0, 0.5]);
        assert_eq!(m.values(99.0)[0], 1.0, "and stays there");
        assert_eq!(m.values(8.0)[0], 0.0, "a clock that jumped back holds it at its start");
        // a knob turned mid-morph: that macro is the knob's, the others go on
        m.set(1, 0.9);
        assert_eq!(m.values(12.0)[..2], [0.5, 0.9]);
        // a new morph starts from where they are now
        let mut back = [0.0; MACROS];
        back[1] = 0.9;
        m.morph(12.0, back, 2.0);
        assert_eq!(m.values(12.0)[0], 0.5);
        assert_eq!(m.values(13.0)[0], 0.25);
        // no beats: at once
        m.morph(13.0, [0.3; MACROS], 0.0);
        assert_eq!(m.values(13.0), [0.3; MACROS]);
    }

    #[test]
    fn taps_set_the_tempo_from_their_average_and_a_pause_restarts() {
        let mut c = Clock::default();
        assert_eq!(c.bpm(), 120.0);
        // a series at 100 BPM, starting at 10 s: the first tap is a downbeat
        for i in 0..5 {
            c.tap(10.0 + 0.6 * f64::from(i));
        }
        assert!((c.bpm() - 100.0).abs() < 1e-3, "{}", c.bpm());
        // just after the fifth tap: the next bar's downbeat
        let [beat, bar, hit] = c.at(12.41);
        assert!(beat < 0.02 && bar < 0.01 && hit > 0.9, "{beat} {bar} {hit}");
        // only the newest 8 taps count: 7 more, 0.5 s apart, push the 0.6-s gaps out
        for i in 1..=7 {
            c.tap(12.4 + 0.5 * f64::from(i));
        }
        assert!((c.bpm() - 120.0).abs() < 1e-3, "the newest 8 taps are 0.5 s apart: {}", c.bpm());
        // after more than 2 s the next tap starts a series: a downbeat, the tempo kept
        c.tap(30.0);
        assert!((c.bpm() - 120.0).abs() < 1e-3);
        assert_eq!(c.at(30.0)[1], 0.0);
        c.tap(30.4);
        assert!((c.bpm() - 150.0).abs() < 1e-3, "a new series' second tap sets the tempo: {}", c.bpm());
    }

    #[test]
    fn the_beat_ramps_through_beat_and_bar_and_tempo_changes_keep_the_phase() {
        let mut c = Clock::default();
        let [beat, bar, hit] = c.at(0.25);
        assert!((beat - 0.5).abs() < 1e-6 && (bar - 0.125).abs() < 1e-6, "{beat} {bar}");
        assert!((hit - (-0.25f32 / BEAT_DECAY).exp()).abs() < 1e-6);
        assert!(c.at(1.75)[1] > 0.87 && c.at(2.0)[1] < 1e-6, "a bar is four beats");
        assert!(c.at(-0.25)[0] > 0.0, "phases stay in 0..1 before beat 0");
        let before = c.at(5.3);
        c.set_bpm(5.3, 140.0);
        let after = c.at(5.3);
        assert!((before[0] - after[0]).abs() < 1e-4 && (before[1] - after[1]).abs() < 1e-4, "{before:?} {after:?}");
        c.set_bpm(5.3, 9999.0);
        assert_eq!(c.bpm(), MAX_BPM);
        c.set_bpm(5.3, f32::NAN);
        assert_eq!(c.bpm(), MAX_BPM);
        c.resync(7.0);
        assert_eq!(c.at(7.0)[..2], [0.0, 0.0]);
    }

    #[test]
    fn an_outside_clock_sets_the_tempo_and_locks_the_beat() {
        let mut c = Clock::default();
        // beat 8 of a 125-BPM clock lands at t = 10 s
        c.follow(10.0, 125.0, Some(8.0));
        assert_eq!(c.bpm(), 125.0);
        assert!((c.beats(10.0) - 8.0).abs() < 1e-9);
        assert!((c.beats(10.48) - 9.0).abs() < 1e-9, "one beat later is beat 9");
        // joined mid-song: the nearest beat, whatever its number
        c.follow(20.01, 125.0, None);
        assert!((c.beats(20.01) - c.beats(20.01).round()).abs() < 1e-9);
        assert_eq!(c.at(20.01)[0], 0.0, "on the beat");
    }

    #[test]
    fn an_lfo_is_a_source_after_the_audio_and_the_clock() {
        let kaleido = shader("kaleido");
        let r = routes(&kaleido, &[mapping("zoom", "n_lfo2", 0.5), mapping("spin", "n_gone", 0.5)], &["n_lfo1/0".into(), "n_lfo2/0".into()]);
        assert_eq!(r.len(), 1, "a gone LFO maps nothing");
        assert_eq!(r[0].source, SOURCES.len() + 1);
    }
}
