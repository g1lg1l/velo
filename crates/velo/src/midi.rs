//! MIDI control (docs/control.md): every input port, re-scanned each second for hot-plug, on a
//! thread of its own; each message is stamped when it arrives and queued for the UI, which
//! applies the project's bindings once a frame (continuous controls coalesced to their last
//! value, every press counted), and learns new ones.
//! From Ravune (03cb268), adapted for Velo.
//!
//! Bindings act through operators (macros and parameters are document data, so undo and saving
//! work as for the mouse) and through the engine's commands (launches, blackout, taps, which
//! carry the time they arrived). The render thread never sees MIDI: it gets the values the
//! next frame's sync sends, a UI frame (≤ 16 ms) after they arrived.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use serde_json::json;
use velo_core::{Behavior, Binding, Control, MidiMsg, ParamInfo, RelMode, Takeover, Target};

use crate::shell::Tone;
use crate::{App, i18n, live};

/// One message as it arrived: its port, when, its channel (0..15), what it is and its value (a
/// CC's value, a note's velocity with 0 for note-off, a program's number).
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub port: Arc<str>,
    pub at: Instant,
    pub channel: u8,
    pub msg: MidiMsg,
    pub value: u8,
}

/// A control's messages over one frame: its last value and where it was before them, every
/// value in order (an encoder's clicks add up), how many presses came (a note-on, a CC rising
/// past the middle, a program change) and when the last did. A 14-bit controller's `fine` is
/// its value over 0..16383 (and `value` its coarse half).
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub port: Arc<str>,
    pub channel: u8,
    pub msg: MidiMsg,
    pub value: u8,
    pub prev: Option<u8>,
    pub values: Vec<u8>,
    pub fine: Option<u16>,
    pub presses: u32,
    pub at: Instant,
}

impl Input {
    /// Where it is along its travel, 0..1: 14 bits when it has them.
    pub fn level(&self) -> f32 {
        self.fine.map_or(f32::from(self.value) / 127.0, |f| f32::from(f) / 16383.0)
    }
}

/// Notes, CCs (not the channel-mode ones, 120 and up) and program changes; the rest (clock,
/// sysex, pressure, pitch bend) isn't bound yet.
pub fn parse(bytes: &[u8]) -> Option<(u8, MidiMsg, u8)> {
    let (&status, data) = bytes.split_first()?;
    let channel = status & 15;
    match (status & 0xF0, data) {
        (0x90, &[note, velocity, ..]) => Some((channel, MidiMsg::Note(note & 127), velocity & 127)),
        (0x80, &[note, ..]) => Some((channel, MidiMsg::Note(note & 127), 0)),
        (0xB0, &[cc, value, ..]) if cc < 120 => Some((channel, MidiMsg::Cc(cc), value & 127)),
        (0xC0, &[program, ..]) => Some((channel, MidiMsg::ProgramChange, program & 127)),
        _ => None,
    }
}

type Key = (Arc<str>, u8, MidiMsg);

/// One frame's events, in order of first arrival: a CC or note becomes one input with its last
/// value and its presses counted (none is lost to coalescing); each program change stays its own.
/// `last` keeps every control's latest value across frames (a CC press is a rise past 64 from
/// below it).
pub fn coalesce(events: impl IntoIterator<Item = Event>, last: &mut HashMap<Key, u8>) -> Vec<Input> {
    let mut out: Vec<Input> = Vec::new();
    for e in events {
        let key = (e.port.clone(), e.channel, e.msg);
        let prev = last.insert(key, e.value);
        let press = match e.msg {
            MidiMsg::Cc(_) | MidiMsg::Cc14(_) => e.value >= 64 && prev.is_none_or(|p| p < 64),
            MidiMsg::Note(_) => e.value > 0,
            MidiMsg::ProgramChange => true,
        };
        // a 14-bit pair (CC n and n + 32, both seen) is also one fine control
        let pair = match e.msg {
            MidiMsg::Cc(n) if n < 32 => Some(n),
            MidiMsg::Cc(n) if (32..64).contains(&n) => Some(n - 32),
            _ => None,
        };
        let fine = pair.and_then(|n| {
            let half = |cc: u8| last.get(&(e.port.clone(), e.channel, MidiMsg::Cc(cc))).copied();
            Some((n, u16::from(half(n)?) << 7 | u16::from(half(n + 32)?)))
        });
        let add = |out: &mut Vec<Input>, msg: MidiMsg, value: u8, prev: Option<u8>, fine: Option<u16>| {
            let same = |i: &&mut Input| i.msg != MidiMsg::ProgramChange && (i.port.as_ref(), i.channel, i.msg) == (e.port.as_ref(), e.channel, msg);
            match out.iter_mut().find(|i| same(i)) {
                Some(i) => {
                    (i.value, i.fine) = (value, fine);
                    i.values.push(value);
                    i.presses += u32::from(press && fine.is_none());
                    if press {
                        i.at = e.at;
                    }
                }
                None => out.push(Input { port: e.port.clone(), channel: e.channel, msg, value, prev, values: vec![value], fine, presses: u32::from(press && fine.is_none()), at: e.at }),
            }
        };
        add(&mut out, e.msg, e.value, prev, None);
        if let Some((n, value)) = fine {
            add(&mut out, MidiMsg::Cc14(n), (value >> 7) as u8, None, Some(value));
        }
    }
    out
}

/// Soft takeover (docs/control.md §2): a knob now at `knob` (0..1), last at `last`, moves a value
/// at `current` only once it reaches it: it's within a step of it, or it crossed it since last
/// time. A knob that set the value last time is on it, so it keeps moving it.
pub fn picks_up(last: Option<f32>, knob: f32, current: f32) -> bool {
    const NEAR: f32 = 1.5 / 127.0;
    (knob - current).abs() <= NEAR || last.is_some_and(|l| (l - current).abs() <= NEAR || (l < current) != (knob < current))
}

/// "C1" for note 36 (middle C, 60, is C3, as Ableton and most controllers' manuals name it).
pub fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[usize::from(n % 12)], i32::from(n / 12) - 2)
}

/// A control as its badge says it: "CC 21", "Note C1", "Program", "Key 4".
pub fn control_label(c: &Control) -> String {
    let msg = match c {
        Control::Midi { msg, .. } => msg,
        Control::Key(name) => return tr!("Key {}", name),
    };
    match msg {
        MidiMsg::Cc(n) => tr!("CC {}", n),
        MidiMsg::Note(n) => tr!("Note {}", note_name(*n)),
        MidiMsg::ProgramChange => tr!("Program").to_string(),
        MidiMsg::Cc14(n) => tr!("CC {} (14-bit)", n),
    }
}

/// An encoder's dialect as its manual would put it.
pub fn encoder_name(mode: RelMode) -> String {
    match mode {
        RelMode::TwosComplement => tr!("Encoder 1/127"),
        RelMode::BinaryOffset => tr!("Encoder 65/63"),
        RelMode::SignMagnitude => tr!("Encoder 1/65"),
    }
    .to_string()
}

/// How a control learned for `target` behaves: a knob sets macros (picking them up after a scene
/// change) and parameters (at once); a key or pad flips them, and fires the rest.
pub fn behavior_for(target: &Target, msg: MidiMsg) -> Behavior {
    match (target, msg) {
        (Target::Macro(_), MidiMsg::Cc(_) | MidiMsg::Cc14(_)) => Behavior::Absolute { takeover: Takeover::Pickup },
        (Target::Param { .. }, MidiMsg::Cc(_) | MidiMsg::Cc14(_)) => Behavior::Absolute { takeover: Takeover::Jump },
        _ => press_behavior(target),
    }
}

/// How a pad or a key learned for `target` behaves: it flips what has two ends and fires the rest.
pub fn press_behavior(target: &Target) -> Behavior {
    match target {
        Target::Macro(_) | Target::Param { .. } | Target::Blackout | Target::Freeze => Behavior::Toggle,
        _ => Behavior::Trigger,
    }
}

/// A key of the computer's keyboard as bindings name it ("4", "A", "F5"), if it can be bound:
/// a letter, a digit or a function key (docs/control.md §3).
pub fn key_name(key: egui::Key) -> Option<&'static str> {
    Some(key.name()).filter(|name| Control::key_name(name))
}

// ---------------------------------------------------------------------------------------------

/// What an outside MIDI clock asks of the beat clock (docs/control.md §4).
#[derive(Clone, Debug, PartialEq)]
pub enum Sync {
    /// Start: a downbeat at this time.
    Start(Instant),
    /// A beat: the tempo measured over the last two beats, its number since Start if Start was
    /// seen, when it came.
    Beat { bpm: f32, beat: Option<u64>, at: Instant },
}

/// Follows a MIDI clock: 24 ticks a beat, Start, Continue, Stop. The tempo comes from up to the
/// last 96 ticks (four beats: a USB port's jitter averages out), the beat count from Start.
#[derive(Default)]
pub struct Follower {
    ticks: VecDeque<Instant>,
    /// Ticks since Start (None until a Start is seen: joined while the clock was running).
    count: Option<u64>,
    /// Ticks seen since joining, for the beats before the first Start.
    seen: u64,
}

impl Follower {
    /// A system real-time message at `at`: what the beat clock should do, if anything.
    pub fn feed(&mut self, status: u8, at: Instant) -> Option<Sync> {
        match status {
            0xFA => {
                (self.count, self.seen) = (Some(0), 0);
                self.ticks.clear();
                Some(Sync::Start(at))
            }
            0xF8 => {
                self.ticks.push_back(at);
                if self.ticks.len() > 97 {
                    self.ticks.pop_front();
                }
                let n = self.count.unwrap_or(self.seen);
                self.seen += 1;
                self.count = self.count.map(|c| c + 1);
                // once a beat, once two beats of ticks tell the tempo
                if !n.is_multiple_of(24) || self.ticks.len() < 49 {
                    return None;
                }
                let span = self.ticks.back()?.duration_since(*self.ticks.front()?).as_secs_f64();
                let bpm = 60.0 / (span / (self.ticks.len() - 1) as f64 * 24.0);
                Some(Sync::Beat { bpm: bpm as f32, beat: self.count.map(|_| n / 24), at })
            }
            // Stop and Continue: the clock just stops or goes on ticking; the count stays
            _ => None,
        }
    }
}

/// The MIDI input from the UI's side: the queue the port thread fills, the ports it has open,
/// and what the UI remembers between frames.
pub struct Midi {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// What MIDI clocks said, and when their last beat came (the tempo chip says so).
    sync_tx: Sender<Sync>,
    sync_rx: Receiver<Sync>,
    pub clock_seen: Option<Instant>,
    ports: Arc<Mutex<Vec<String>>>,
    /// Every control's last value (presses, soft takeover).
    last: HashMap<Key, u8>,
    /// When the UI last took something from the queue (the chip's blink), and when a binding
    /// last moved a value (a pause ends its undo step).
    pub seen: f64,
    moved: f64,
}

impl Default for Midi {
    /// The queue alone, no ports: what tests inject into.
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let (sync_tx, sync_rx) = mpsc::channel();
        Self { tx, rx, sync_tx, sync_rx, clock_seen: None, ports: Arc::default(), last: HashMap::new(), seen: f64::NEG_INFINITY, moved: f64::NEG_INFINITY }
    }
}

impl Midi {
    /// Opens every port, now and whenever one is plugged in, on a thread that stops with this.
    pub fn start(ctx: &egui::Context) -> Self {
        let midi = Self::default();
        let (tx, sync, ports, ctx) = (midi.tx.clone(), midi.sync_tx.clone(), midi.ports.clone(), ctx.clone());
        let _ = std::thread::Builder::new().name("velo midi".into()).spawn(move || watch(&tx, &sync, &ports, &ctx));
        midi
    }

    /// Where events come in: the port thread's queue, which tests inject into.
    #[cfg(test)]
    pub fn sender(&self) -> Sender<Event> {
        self.tx.clone()
    }

    /// The open ports' names, as last scanned.
    pub fn ports(&self) -> Vec<String> {
        self.ports.try_lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Everything that came since the last frame, coalesced; never waits.
    fn take(&mut self) -> Vec<Input> {
        coalesce(self.rx.try_iter(), &mut self.last)
    }
}

/// The port thread: a scan a second, connecting new ports and closing gone ones (dropping a
/// connection closes it). Ends once the UI's side is gone.
fn watch(tx: &Sender<Event>, sync: &Sender<Sync>, ports: &Arc<Mutex<Vec<String>>>, ctx: &egui::Context) {
    use midir::{Ignore, MidiInput, MidiInputConnection};
    let mut open: Vec<(String, MidiInputConnection<()>)> = Vec::new();
    while Arc::strong_count(ports) > 1 {
        if let Ok(scanner) = MidiInput::new("Velo") {
            let found: Vec<_> = scanner.ports().into_iter().filter_map(|p| Some((scanner.port_name(&p).ok()?, p))).collect();
            open.retain(|(name, _)| found.iter().any(|(n, _)| n == name));
            for (name, port) in found {
                if open.iter().any(|(n, _)| *n == name) {
                    continue;
                }
                let Ok(mut input) = MidiInput::new("Velo") else { continue };
                // the clock comes through (sysex and active sensing don't)
                input.ignore(Ignore::SysexAndActiveSense);
                let (tx, sync, ctx, port_name) = (tx.clone(), sync.clone(), ctx.clone(), Arc::<str>::from(name.as_str()));
                let mut follower = Follower::default();
                let on_message = move |_: u64, bytes: &[u8], _: &mut ()| {
                    if let Some(&status) = bytes.first().filter(|&&s| s >= 0xF8) {
                        if let Some(s) = follower.feed(status, Instant::now()) {
                            let _ = sync.send(s);
                            ctx.request_repaint();
                        }
                    } else if let Some((channel, msg, value)) = parse(bytes) {
                        let _ = tx.send(Event { port: port_name.clone(), at: Instant::now(), channel, msg, value });
                        ctx.request_repaint();
                    }
                };
                // a port another app holds may refuse: tried again next scan
                if let Ok(connection) = input.connect(&port, "velo-in", on_message, ()) {
                    open.push((name, connection));
                }
            }
            if let Ok(mut p) = ports.lock() {
                *p = open.iter().map(|(n, _)| n.clone()).collect();
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// MIDI learn (docs/control.md §3): learn mode outlines what can be bound; the control picked
/// waits for the first meaningful message (a CC moved by 2 steps, a note-on, a program change).
#[derive(Default)]
pub struct Learn {
    pub on: bool,
    pub target: Option<Target>,
    /// Where each CC was when the wait began, and what each sent since (encoders tell).
    from: HashMap<Key, u8>,
    seen: HashMap<Key, Vec<u8>>,
}

impl Learn {
    /// Waits for a controller for `target`.
    pub fn wait(&mut self, target: Target) {
        self.target = Some(target);
        self.from.clear();
        self.seen.clear();
    }

    pub fn stop(&mut self) {
        (self.on, self.target) = (false, None);
    }
}

// ---------------------------------------------------------------------------------------------

/// A slider's description, which float of the vec4 and the stored vec4 of a node's parameter
/// (by its mapping name), with the scene it's in.
pub struct ParamRef {
    pub scene: String,
    pub id: String,
    pub info: &'static ParamInfo,
    pub axis: usize,
    pub value: [f32; 4],
}

impl App {
    /// Once a frame: what came from the controllers is learned or does what it's bound to.
    pub fn midi_frame(&mut self, now: f64) {
        // an outside MIDI clock drives the beat clock: its tempo, its beats, its Start
        let syncs: Vec<Sync> = self.midi.sync_rx.try_iter().collect();
        for s in syncs {
            self.midi.clock_seen = Some(Instant::now());
            if let Some(live) = &self.live {
                match s {
                    Sync::Start(at) => live.resync_at(at),
                    Sync::Beat { bpm, beat, at } => live.follow(bpm, beat.map(|b| b as f64), at),
                }
            }
        }
        let mut inputs = self.midi.take();
        // learning: a 14-bit pair is learned whole, before its coarse and fine halves
        if self.learn.target.is_some() {
            inputs.sort_by_key(|i| !matches!(i.msg, MidiMsg::Cc14(_)));
        }
        if !inputs.is_empty() {
            self.midi.seen = now;
        }
        for input in inputs {
            if self.learn_from(&input) {
                continue;
            }
            let bound: Vec<Binding> = self.session.project.bindings.iter().filter(|b| matches(&b.control, &input)).cloned().collect();
            for b in bound {
                self.fire(&b, &input, now);
            }
        }
    }

    /// While a control waits to be learned, the first meaningful message binds it. True when
    /// the input was learn's.
    fn learn_from(&mut self, input: &Input) -> bool {
        let Some(target) = self.learn.target.clone() else { return false };
        let key = (input.port.clone(), input.channel, input.msg);
        // an endless encoder: a few small steps around 0 or 64, never a knob's travel
        let seen = self.learn.seen.entry(key.clone()).or_default();
        seen.extend(&input.values);
        let encoder = matches!(input.msg, MidiMsg::Cc(_)).then(|| RelMode::guess(seen)).flatten();
        let meaningful = match input.msg {
            _ if encoder.is_some() => true,
            MidiMsg::Cc(_) | MidiMsg::Cc14(_) => {
                let from = *self.learn.from.entry(key).or_insert(input.prev.unwrap_or(input.value));
                input.value.abs_diff(from) >= 2
            }
            MidiMsg::Note(_) | MidiMsg::ProgramChange => input.presses > 0,
        };
        if !meaningful {
            return true;
        }
        // a program change picks the scene by its number
        let target = if input.msg == MidiMsg::ProgramChange && matches!(target, Target::LaunchScene(_)) { Target::LaunchScene(0) } else { target };
        let control = Control::Midi { port: Some(input.port.to_string()), channel: input.channel, msg: input.msg };
        let behavior = match encoder {
            Some(mode) if matches!(target, Target::Macro(_) | Target::Param { .. }) => Behavior::Relative { mode, step: 1.0 / 128.0 },
            _ => behavior_for(&target, input.msg),
        };
        self.bind(Binding { behavior, control, target });
        true
    }

    /// What learn found for the control that waited: bound, and said.
    fn bind(&mut self, binding: Binding) {
        let text = tr!("{} now drives {}", control_label(&binding.control), self.target_name(&binding.target));
        self.run("binding.add", json!(binding));
        self.say(Tone::Done, text);
        self.learn.target = None;
    }

    /// Whether a key pressed alone is the project's, not the keymap's (docs/control.md §3): a
    /// control waits to learn it, or a binding has it. In learn mode the key that leaves it
    /// (`L`) stays the keymap's, so there is always a way out.
    pub fn takes_key(&self, key: egui::Key) -> bool {
        let Some(name) = key_name(key) else { return false };
        let learning = self.learn.on || self.learn.target.is_some();
        if learning && self.settings.keymap.keys("stage.learn").iter().any(|k| k.modifiers.is_none() && k.logical_key == key) {
            return false;
        }
        self.learn.target.is_some() || self.session.project.bindings.iter().any(|b| matches!(&b.control, Control::Key(k) if k == name))
    }

    /// A key `takes_key` took, pressed: it binds the control that waits (and does nothing else),
    /// or does what it's bound to, as a pad would.
    pub fn key_press(&mut self, key: egui::Key, now: f64) {
        let Some(control) = key_name(key).map(|name| Control::Key(name.into())) else { return };
        if let Some(target) = self.learn.target.clone() {
            return self.bind(Binding { behavior: press_behavior(&target), control, target });
        }
        let bound: Vec<Target> = self.session.project.bindings.iter().filter(|b| b.control == control).map(|b| b.target.clone()).collect();
        for target in bound {
            self.trigger(&target, Instant::now(), now);
        }
    }

    /// The scene macros and variations from controllers go to: the one on the output (the
    /// selected one without an engine).
    pub fn stage_scene(&self) -> Option<String> {
        self.live.as_ref().and_then(|l| l.live_scene().map(String::from)).filter(|id| self.session.project.scene(id).is_some()).or_else(|| self.session.active_scene.clone())
    }

    /// A node's parameter by its mapping name, wherever it is in the project.
    pub fn param_ref(&self, node: &str, key: &str) -> Option<ParamRef> {
        let scene = self.session.project.graphs().find(|s| s.node(node).is_some())?;
        let n = scene.node(node)?;
        let (id, axis) = match key.rsplit_once('.') {
            Some((id, "x")) => (id, 0),
            Some((id, "y")) => (id, 1),
            _ => (key, 0),
        };
        let shader = live::shader(&n.kind, n.code.as_deref())?;
        let (param, infos) = shader.controls.iter().find(|(p, _)| p.id == id)?;
        let info = infos.get(axis)?;
        Some(ParamRef { scene: scene.id.clone(), id: id.to_string(), info, axis, value: n.params.get(id).copied().unwrap_or(param.default) })
    }

    /// Sets a parameter's value (in its units). One driven by a macro moves the macro instead,
    /// so the macro stays the one source of its value.
    pub fn set_param(&mut self, node: &str, key: &str, v: f32, merge: &str) {
        let Some(p) = self.param_ref(node, key) else { return };
        let driver = self.session.project.scene(&p.scene).and_then(|s| s.driver(node, key)).map(|(i, t)| (i, t.clone()));
        match driver {
            Some((index, t)) if (t.max - t.min).abs() > f32::EPSILON => {
                self.run_merge("macro.set", json!({ "scene": p.scene, "index": index, "value": ((v - t.min) / (t.max - t.min)).clamp(0.0, 1.0) }), merge);
            }
            Some(_) => {}
            None => {
                let mut value = p.value;
                value[p.axis] = p.info.clamp(v);
                self.run_merge("node.set_param", json!({ "scene": p.scene, "node": node, "param": p.id, "value": value }), merge);
            }
        }
    }

    /// Where a continuous target is along its travel (0..1): what soft takeover compares a knob with.
    pub(crate) fn travel(&self, target: &Target) -> Option<f32> {
        match target {
            Target::Macro(i) => self.session.project.scene(&self.stage_scene()?)?.macros.get(usize::from(*i)).map(|m| m.value),
            Target::Param { node, param } => {
                let p = self.param_ref(node, param)?;
                let scene = self.session.project.scene(&p.scene)?;
                let v = scene.driver(node, param).map_or(p.value[p.axis], |(i, t)| t.at(scene.macros[i].value));
                Some(p.info.to_norm(v))
            }
            Target::Blackout => Some(f32::from(u8::from(self.live.as_ref().is_some_and(|l| l.blackout)))),
            Target::Freeze => Some(f32::from(u8::from(self.live.as_ref().is_some_and(|l| l.frozen)))),
            _ => None,
        }
    }

    /// Moves a continuous target to `k` (0..1) of its travel. Moves from controllers in a row
    /// are one undo step; a second's pause starts the next.
    pub(crate) fn set_travel(&mut self, target: &Target, k: f32, now: f64) {
        if now - self.midi.moved > 1.0 {
            self.session.close_merge();
        }
        self.midi.moved = now;
        match target {
            Target::Macro(i) => {
                if let Some(scene) = self.stage_scene() {
                    self.run_merge("macro.set", json!({ "scene": scene, "index": i, "value": k }), "midi");
                }
            }
            Target::Param { node, param } => {
                if let Some(p) = self.param_ref(node, param) {
                    self.set_param(node, param, p.info.from_norm(k), "midi");
                }
            }
            Target::Blackout => {
                if let Some(live) = &mut self.live {
                    live.set_blackout(k >= 0.5);
                }
            }
            Target::Freeze => {
                if let Some(live) = &mut self.live {
                    live.set_freeze(k >= 0.5);
                }
            }
            _ => {}
        }
    }

    /// One binding, one control's frame of messages.
    fn fire(&mut self, b: &Binding, input: &Input, now: f64) {
        let k = input.level();
        match b.behavior {
            Behavior::Relative { mode, step } => {
                let clicks: i32 = input.values.iter().map(|&v| mode.clicks(v)).sum();
                if let (Some(current), true) = (self.travel(&b.target), clicks != 0) {
                    self.set_travel(&b.target, (current + clicks as f32 * step).clamp(0.0, 1.0), now);
                }
            }
            Behavior::Absolute { takeover } if input.msg != MidiMsg::ProgramChange => {
                let Some(current) = self.travel(&b.target) else { return self.press(b, input, now) };
                let last = if input.fine.is_some() { None } else { input.prev.map(|p| f32::from(p) / 127.0) };
                if takeover == Takeover::Jump || picks_up(last, k, current) {
                    self.set_travel(&b.target, k, now);
                }
            }
            _ => self.press(b, input, now),
        }
    }

    /// A press, as many times as it came: continuous targets flip between their ends, the
    /// rest fire.
    fn press(&mut self, b: &Binding, input: &Input, now: f64) {
        for _ in 0..input.presses {
            // a program change picks the scene by its number
            match &b.target {
                Target::LaunchScene(i) if input.msg == MidiMsg::ProgramChange => self.trigger(&Target::LaunchScene(i + usize::from(input.value)), input.at, now),
                target => self.trigger(target, input.at, now),
            }
        }
    }

    /// One press on `target` (MIDI, OSC): continuous targets flip between their ends, the rest
    /// fire; `at` is when it came (a tap's time).
    pub(crate) fn trigger(&mut self, target: &Target, at: Instant, now: f64) {
        if let Some(k) = self.travel(target) {
            self.set_travel(target, if k >= 0.5 { 0.0 } else { 1.0 }, now);
            return;
        }
        match target {
            Target::LaunchScene(n) => {
                if let Some(scene) = self.session.project.scenes.get(*n).map(|s| s.id.clone()) {
                    self.launch(&scene);
                }
            }
            Target::LaunchSelected => self.command("stage.launch"),
            Target::Variation(i) => {
                if let Some(scene) = self.stage_scene() {
                    self.run("variation.recall", json!({ "scene": scene, "index": i }));
                }
            }
            Target::TapTempo => {
                self.settings.auto_bpm = false;
                if let Some(live) = &self.live {
                    live.tap_at(at);
                }
            }
            _ => {}
        }
    }

    /// A control MIDI can drive: its controller's badge, and in learn mode an outline; a click
    /// there picks it to learn. True when the click was learn's (the control's own action
    /// doesn't happen).
    pub fn learnable(&mut self, ui: &egui::Ui, resp: &egui::Response, target: &Target) -> bool {
        let waiting = self.learn.target.as_ref() == Some(target);
        if self.learn.on || waiting {
            crate::widgets::learn_ring(ui, &self.theme, resp.rect, waiting);
        }
        if let Some(label) = self.binding_label(target) {
            crate::widgets::control_badge(ui, &self.theme, resp.rect.right_top(), &label);
        }
        if self.learn.on && resp.clicked() {
            self.learn.wait(target.clone());
            return true;
        }
        false
    }

    /// "Learn MIDI or a key" and, once bound, "Forget CC 21": for a control's right-click menu.
    pub fn learn_items(&mut self, ui: &mut egui::Ui, target: &Target) {
        let waiting = self.learn.target.as_ref() == Some(target);
        let label = if waiting { tr!("Waiting for a controller or a key…") } else { tr!("Learn MIDI or a key") };
        if ui.button(format!("{}  {label}", crate::icons::CROSSHAIR)).clicked() {
            self.learn.wait(target.clone());
            self.say(Tone::Action, tr!("Move a knob or hit a pad on your controller, or press a key (Esc cancels)"));
            ui.close();
        }
        if let Some(label) = self.binding_label(target)
            && ui.button(format!("{}  {}", crate::icons::X, tr!("Forget {}", label))).clicked()
        {
            self.run("binding.remove", json!({ "target": target }));
            ui.close();
        }
    }

    /// What a binding drives, as the bindings list says it.
    pub fn target_name(&self, target: &Target) -> String {
        match target {
            Target::Macro(i) => tr!("Macro {}", i + 1),
            Target::Param { node, param } => match (self.session.project.graphs().find_map(|s| s.node(node)), self.param_ref(node, param)) {
                (Some(n), Some(p)) => format!("{} · {}", n.name, i18n::t(p.info.name)),
                _ => tr!("a deleted parameter").to_string(),
            },
            Target::LaunchScene(i) => tr!("Launch scene {}", i + 1),
            Target::LaunchSelected => tr!("Launch the selected scene").to_string(),
            Target::Variation(i) => tr!("Variation {}", variation_letter(*i)),
            Target::Blackout => tr!("Blackout").to_string(),
            Target::Freeze => tr!("Freeze").to_string(),
            Target::TapTempo => tr!("Tap tempo").to_string(),
        }
    }

    /// The badge for whatever drives `target`, if a control does.
    pub fn binding_label(&self, target: &Target) -> Option<String> {
        self.session.project.binding(target).map(|b| control_label(&b.control))
    }
}

/// Whether an input is a binding's control: the same message on the same channel, from its
/// port or from any port.
fn matches(control: &Control, input: &Input) -> bool {
    let Control::Midi { port, channel, msg } = control else { return false };
    *channel == input.channel && *msg == input.msg && port.as_deref().is_none_or(|p| p == input.port.as_ref())
}

/// "A" for variation 0.
pub fn variation_letter(i: usize) -> char {
    (b'A' + (i % 26) as u8) as char
}

/// Macro n's name as the interface says it: its own, or "Macro n".
pub fn macro_name(m: &velo_core::Macro, i: usize) -> String {
    if m.name.is_empty() { tr!("Macro {}", i + 1) } else { m.name.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(msg: MidiMsg, value: u8) -> Event {
        Event { port: "Pads".into(), at: Instant::now(), channel: 0, msg, value }
    }

    #[test]
    fn notes_ccs_and_program_changes_parse_with_their_channel() {
        assert_eq!(parse(&[0x93, 36, 100]), Some((3, MidiMsg::Note(36), 100)));
        assert_eq!(parse(&[0x93, 36, 0]), Some((3, MidiMsg::Note(36), 0)), "a note-on at 0 is a note-off");
        assert_eq!(parse(&[0x80, 36, 64]), Some((0, MidiMsg::Note(36), 0)));
        assert_eq!(parse(&[0xB1, 21, 127]), Some((1, MidiMsg::Cc(21), 127)));
        assert_eq!(parse(&[0xC9, 4]), Some((9, MidiMsg::ProgramChange, 4)));
        assert_eq!(parse(&[0xB0, 123, 0]), None, "all notes off is a channel-mode message");
        assert_eq!(parse(&[0xF8]), None, "clock: the follower's, not a control");
        assert_eq!(parse(&[0xE0, 0, 64]), None, "pitch bend isn't bound yet");
        assert_eq!(parse(&[0x90, 36]), None, "a short message");
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn a_frame_keeps_each_knobs_last_value_and_counts_every_press() {
        let mut last = HashMap::new();
        let frame = [
            event(MidiMsg::Cc(21), 10),
            event(MidiMsg::Cc(21), 12),
            event(MidiMsg::Cc(21), 15),
            // a pad sending CCs: press, release, press, release
            event(MidiMsg::Cc(40), 127),
            event(MidiMsg::Cc(40), 0),
            event(MidiMsg::Cc(40), 127),
            event(MidiMsg::Cc(40), 0),
            event(MidiMsg::Note(36), 90),
            event(MidiMsg::Note(36), 0),
            event(MidiMsg::Note(36), 70),
            event(MidiMsg::ProgramChange, 2),
            event(MidiMsg::ProgramChange, 5),
            Event { channel: 1, ..event(MidiMsg::Cc(21), 99) },
        ];
        let inputs = coalesce(frame, &mut last);
        let seen: Vec<(u8, MidiMsg, u8, Option<u8>, u32)> = inputs.iter().map(|i| (i.channel, i.msg, i.value, i.prev, i.presses)).collect();
        assert_eq!(
            seen,
            [
                (0, MidiMsg::Cc(21), 15, None, 0),
                (0, MidiMsg::Cc(40), 0, None, 2),
                (0, MidiMsg::Note(36), 70, None, 2),
                (0, MidiMsg::ProgramChange, 2, None, 1),
                (0, MidiMsg::ProgramChange, 5, Some(2), 1),
                // first heard above the middle: a pad's first press
                (1, MidiMsg::Cc(21), 99, None, 1),
            ],
            "one knob value per frame, no press lost, each program its own"
        );
        // the next frame knows where things were: a knob from 15, a pad held high isn't a new press
        let inputs = coalesce([event(MidiMsg::Cc(21), 16), event(MidiMsg::Cc(40), 127), event(MidiMsg::Cc(40), 120)], &mut last);
        assert_eq!(inputs.iter().map(|i| (i.prev, i.presses)).collect::<Vec<_>>(), [(Some(15), 0), (Some(0), 1)]);
        let inputs = coalesce([event(MidiMsg::Cc(40), 127)], &mut last);
        assert_eq!(inputs[0].presses, 0, "still held");
        // another port is another control
        let inputs = coalesce([Event { port: "Knobs".into(), ..event(MidiMsg::Cc(21), 16) }], &mut last);
        assert_eq!(inputs[0].prev, None);
    }

    #[test]
    fn encoders_add_their_clicks_and_14_bit_pairs_come_whole() {
        assert_eq!(RelMode::TwosComplement.clicks(1), 1);
        assert_eq!(RelMode::TwosComplement.clicks(126), -2);
        assert_eq!(RelMode::BinaryOffset.clicks(63), -1);
        assert_eq!(RelMode::SignMagnitude.clicks(66), -2);
        assert_eq!(RelMode::guess(&[1, 1, 127]), Some(RelMode::TwosComplement));
        assert_eq!(RelMode::guess(&[65, 66, 63]), Some(RelMode::BinaryOffset));
        assert_eq!(RelMode::guess(&[40, 42, 45]), None, "a knob's travel");
        assert_eq!(RelMode::guess(&[1, 1]), None, "too few to tell");
        // a frame of an encoder's clicks: every value kept
        let mut last = HashMap::new();
        let frame = coalesce([event(MidiMsg::Cc(10), 1), event(MidiMsg::Cc(10), 1), event(MidiMsg::Cc(10), 127)], &mut last);
        assert_eq!(frame[0].values, [1, 1, 127]);
        // a 14-bit fader: CC 7 then CC 39 make one fine control
        let frame = coalesce([event(MidiMsg::Cc(7), 64), event(MidiMsg::Cc(39), 32)], &mut last);
        let fine = frame.iter().find(|i| i.msg == MidiMsg::Cc14(7)).expect("the pair");
        assert_eq!(fine.fine, Some(64 << 7 | 32));
        assert!((fine.level() - (8224.0 / 16383.0)).abs() < 1e-6);
        assert_eq!(fine.presses, 0, "a fine control never presses");
    }

    #[test]
    fn a_midi_clock_gives_tempo_beats_and_the_downbeat() {
        let t0 = Instant::now();
        let tick = Duration::from_secs_f64(60.0 / 125.0 / 24.0); // 125 BPM
        let mut f = Follower::default();
        assert_eq!(f.feed(0xFA, t0), Some(Sync::Start(t0)));
        let mut beats = Vec::new();
        for i in 0..24 * 4 {
            if let Some(Sync::Beat { bpm, beat, .. }) = f.feed(0xF8, t0 + tick * i) {
                assert!((bpm - 125.0).abs() < 0.01, "{bpm}");
                beats.push(beat);
            }
        }
        assert_eq!(beats, [Some(2), Some(3)], "from the third beat, counted from Start");
        // joined while running: beats, but no count
        let mut g = Follower::default();
        let late: Vec<_> = (0..24 * 3).filter_map(|i| g.feed(0xF8, t0 + tick * i)).collect();
        assert!(matches!(late[..], [Sync::Beat { beat: None, .. }]));
        assert_eq!(g.feed(0xFC, t0), None, "Stop changes nothing");
    }

    #[test]
    fn a_knob_picks_the_value_up_once_it_reaches_it() {
        // the macro is at 0.7 (a scene launch put it there), the knob at 0.2
        assert!(!picks_up(None, 0.2, 0.7), "the first move, far off: wait");
        assert!(!picks_up(Some(0.2), 0.3, 0.7), "moving toward it");
        assert!(picks_up(Some(0.6), 0.75, 0.7), "crossed it");
        assert!(picks_up(Some(0.9), 0.65, 0.7), "crossed it from above");
        assert!(picks_up(None, 0.705, 0.7), "within a step of it");
        // once on it the knob drives: the value is where the knob last put it
        assert!(picks_up(Some(0.75), 0.8, 0.75));
        assert!(picks_up(Some(0.75), 0.1, 0.75), "even a jump");
        // the value moved elsewhere (a variation): wait again
        assert!(!picks_up(Some(0.75), 0.8, 0.3));
    }

    #[test]
    fn badges_name_controls_as_controllers_do() {
        assert_eq!(note_name(36), "C1");
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(0), "C-2");
        assert_eq!(note_name(127), "G8");
        assert_eq!(control_label(&Control::Midi { port: None, channel: 0, msg: MidiMsg::Cc(21) }), "CC 21");
        assert_eq!(control_label(&Control::Midi { port: None, channel: 0, msg: MidiMsg::Note(37) }), "Note C#1");
        assert_eq!(control_label(&Control::Key("4".into())), "Key 4");
        // the keys that bind: letters, digits, function keys; never the ones that move around
        use egui::Key;
        assert_eq!([Key::Num4, Key::A, Key::F5].map(key_name), [Some("4"), Some("A"), Some("F5")]);
        assert_eq!([Key::Escape, Key::Space, Key::Tab, Key::Enter, Key::ArrowUp, Key::Delete, Key::F13, Key::Minus].map(key_name), [None; 8]);
        assert_eq!(press_behavior(&Target::Macro(0)), Behavior::Toggle);
        assert_eq!(press_behavior(&Target::Variation(0)), Behavior::Trigger);
        assert_eq!(variation_letter(2), 'C');
        // learned behaviors: knobs set, pads flip continuous targets and fire the rest
        assert_eq!(behavior_for(&Target::Macro(0), MidiMsg::Cc(1)), Behavior::Absolute { takeover: Takeover::Pickup });
        assert_eq!(behavior_for(&Target::Param { node: "n".into(), param: "zoom".into() }, MidiMsg::Cc(1)), Behavior::Absolute { takeover: Takeover::Jump });
        assert_eq!(behavior_for(&Target::Blackout, MidiMsg::Note(1)), Behavior::Toggle);
        assert_eq!(behavior_for(&Target::LaunchScene(2), MidiMsg::Note(1)), Behavior::Trigger);
        assert_eq!(behavior_for(&Target::TapTempo, MidiMsg::Cc(1)), Behavior::Trigger);
    }
}
