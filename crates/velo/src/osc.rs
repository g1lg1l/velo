//! OSC in (docs/control.md §6): a UDP port (9000 by default; off until Preferences → OSC) with
//! an address space under /velo that follows the project, for TouchOSC on a phone, a lighting
//! desk or any tool in any language. Messages are stamped as they arrive on their own thread and
//! applied on the UI thread once a frame, through the same actions as MIDI (faders coalesce to
//! their last value, every trigger counts).
//!
//! ```text
//! /velo/scene/macro/<n>          f 0..1   macro n (1-8) of the scene on the output
//! /velo/scene/launch             i | s    a scene by number (1…) or name
//! /velo/scene/select             i | s    the scene shown in the preview
//! /velo/variation/<n>                     recall variation n (1 = A)
//! /velo/param/<node>/<param>     f 0..1   along the parameter's range; node by id or name
//! /velo/tempo/tap                         tap tempo
//! /velo/tempo/bpm                f
//! /velo/tempo/resync                      a downbeat now
//! /velo/blackout                 [i|f|T|F] on or off; nothing flips it
//! /velo/op/<operator>            [s]      any operator, its props as JSON (only when allowed)
//! /any/other/address             [f]      what a binding learned for it does
//! ```
//!
//! The port listens on every interface (a phone on the Wi-Fi sends to it), so anyone on the
//! network can play the show; `/velo/op`, which can change or delete anything, is off until
//! Preferences → Control allows it.
//!
//! OSC out sends the values Preferences → Control lists (the scene on the output, the tempo, the
//! beat, macros, parameters, signal outputs) to a host and port, as they change or at a fixed
//! rate: read on the UI thread, sent from a thread of its own.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use velo_core::{Behavior, Binding, Control, MidiMsg, Takeover, Target};

use crate::shell::Tone;
use crate::theme::Theme;
use crate::{App, icons, kit, midi};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub enabled: bool,
    pub port: u16,
    /// `/velo/op/<operator>` runs: anything can change, from anyone on the network.
    pub ops: bool,
    /// OSC out: on, to which host and port, how many times a second (0: each value as it
    /// changes), and which values, an address a line.
    pub send: bool,
    pub send_host: String,
    pub send_port: u16,
    pub send_rate: u32,
    pub send_values: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { enabled: false, port: 9000, ops: false, send: false, send_host: "127.0.0.1".into(), send_port: 8000, send_rate: 0, send_values: "/velo/scene/live\n/velo/tempo/bpm\n/velo/tempo/beat".into() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    /// Nil, impulse, a blob: there, but no value.
    Other,
}

impl Arg {
    fn number(&self) -> Option<f64> {
        match self {
            Arg::Int(i) => Some(*i as f64),
            Arg::Float(f) => Some(*f),
            Arg::Bool(b) => Some(f64::from(u8::from(*b))),
            Arg::Str(s) => s.trim().parse().ok(),
            Arg::Other => None,
        }
    }
}

/// One message as it came.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub address: String,
    pub args: Vec<Arg>,
    pub at: Instant,
}

/// A padded OSC string at `*at`, moving past it.
fn string(b: &[u8], at: &mut usize) -> Option<String> {
    let len = b.get(*at..)?.iter().position(|&c| c == 0)?;
    let s = String::from_utf8_lossy(&b[*at..*at + len]).into_owned();
    *at += (len + 4) & !3;
    Some(s)
}

fn word<const N: usize>(b: &[u8], at: &mut usize) -> Option<[u8; N]> {
    let w = b.get(*at..*at + N)?.try_into().ok()?;
    *at += N;
    Some(w)
}

/// A packet's messages: one, or a bundle's (nested bundles too). What doesn't parse is dropped.
pub fn decode(b: &[u8]) -> Vec<(String, Vec<Arg>)> {
    let mut out = Vec::new();
    decode_into(b, &mut out, 0);
    out
}

fn decode_into(b: &[u8], out: &mut Vec<(String, Vec<Arg>)>, depth: usize) {
    if b.starts_with(b"#bundle\0") {
        // the time tag is ignored: everything applies on the next frame
        let mut at = 16;
        while depth < 8
            && let Some(size) = word::<4>(b, &mut at).map(|w| u32::from_be_bytes(w) as usize)
        {
            let Some(element) = b.get(at..at + size) else { break };
            decode_into(element, out, depth + 1);
            at += size;
        }
        return;
    }
    let mut at = 0;
    let Some(address) = string(b, &mut at).filter(|a| a.starts_with('/')) else { return };
    let tags = if b.get(at) == Some(&b',') { string(b, &mut at).unwrap_or_default() } else { String::new() };
    let mut args = Vec::new();
    for tag in tags.chars().skip(1) {
        let arg = match tag {
            'i' => word::<4>(b, &mut at).map(|w| Arg::Int(i64::from(i32::from_be_bytes(w)))),
            'f' => word::<4>(b, &mut at).map(|w| Arg::Float(f64::from(f32::from_be_bytes(w)))),
            'h' => word::<8>(b, &mut at).map(|w| Arg::Int(i64::from_be_bytes(w))),
            'd' => word::<8>(b, &mut at).map(|w| Arg::Float(f64::from_be_bytes(w))),
            's' | 'S' => string(b, &mut at).map(Arg::Str),
            'T' => Some(Arg::Bool(true)),
            'F' => Some(Arg::Bool(false)),
            'b' => word::<4>(b, &mut at).map(|w| {
                at += (u32::from_be_bytes(w) as usize + 3) & !3;
                Arg::Other
            }),
            't' => word::<8>(b, &mut at).map(|_| Arg::Other),
            'c' | 'r' | 'm' => word::<4>(b, &mut at).map(|_| Arg::Other),
            _ => Some(Arg::Other),
        };
        match arg {
            Some(a) => args.push(a),
            None => return, // cut short
        }
    }
    out.push((address, args));
}

/// One message as a packet (what TouchOSC sends).
pub fn message(address: &str, args: &[Arg]) -> Vec<u8> {
    let pad = |b: &mut Vec<u8>, s: &[u8]| {
        b.extend_from_slice(s);
        b.push(0);
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
    };
    let mut b = Vec::new();
    pad(&mut b, address.as_bytes());
    let tags: String = std::iter::once(',')
        .chain(args.iter().map(|a| match a {
            Arg::Int(_) => 'i',
            Arg::Float(_) => 'f',
            Arg::Str(_) => 's',
            Arg::Bool(true) => 'T',
            Arg::Bool(false) => 'F',
            Arg::Other => 'N',
        }))
        .collect();
    pad(&mut b, tags.as_bytes());
    for a in args {
        match a {
            Arg::Int(i) => b.extend_from_slice(&(*i as i32).to_be_bytes()),
            Arg::Float(f) => b.extend_from_slice(&(*f as f32).to_be_bytes()),
            Arg::Str(s) => pad(&mut b, s.as_bytes()),
            _ => {}
        }
    }
    b
}

/// The local port OSC out sends from (0: off): what comes in from it is Velo's own, sent to
/// itself, never an input.
static OUT_PORT: AtomicU16 = AtomicU16::new(0);

/// OSC out: the queue to its thread (none while off) and where that sends, and what went out last.
#[derive(Default)]
pub struct Out {
    tx: Option<Sender<Vec<u8>>>,
    to: (String, u16),
    /// Each address's last value sent (only changes go out), and when everything goes out
    /// again at a fixed rate.
    sent: HashMap<String, Arg>,
    next: f64,
}

impl Out {
    /// Sends the packets it's handed to `host:port` from a thread of its own, which looks the
    /// name up (never the UI's: a `.local` name can take seconds).
    fn start(host: &str, port: u16) -> std::io::Result<Sender<Vec<u8>>> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        OUT_PORT.store(socket.local_addr()?.port(), Relaxed);
        let (tx, rx) = channel::<Vec<u8>>();
        let host = host.to_string();
        std::thread::Builder::new().name("velo osc out".into()).spawn(move || {
            // shortcut: IPv4 only (the socket's), add a v6 socket when a host has only v6
            let Some(to) = (host.as_str(), port).to_socket_addrs().ok().and_then(|mut a| a.find(SocketAddr::is_ipv4)) else { return };
            for packet in rx {
                let _ = socket.send_to(&packet, to);
            }
        })?;
        Ok(tx)
    }
}

impl Drop for Out {
    fn drop(&mut self) {
        if self.tx.is_some() {
            OUT_PORT.store(0, Relaxed);
        }
    }
}

/// The port's thread and what it heard.
pub struct Osc {
    port: u16,
    rx: Receiver<Message>,
    stop: Arc<AtomicBool>,
    /// The last address and when it came (Preferences shows it), and whether Velo knew it.
    pub last: Option<(String, Instant, bool)>,
}

impl Drop for Osc {
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
    }
}

impl Osc {
    /// Listens on `port` on every interface (a phone on the same network sends to it).
    pub fn start(port: u16, ctx: &egui::Context) -> Result<Self, String> {
        let socket = UdpSocket::bind(("0.0.0.0", port)).map_err(|e| e.to_string())?;
        socket.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (flag, ctx) = (stop.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("velo osc".into())
            .spawn(move || {
                let mut buf = vec![0u8; 65_536];
                while !flag.load(Relaxed) {
                    let Ok((n, from)) = socket.recv_from(&mut buf) else { continue };
                    // shortcut: told by the port alone, so a sender elsewhere on the same port
                    // number is dropped too; check the address as well if that ever bites
                    if from.port() == OUT_PORT.load(Relaxed) {
                        continue;
                    }
                    let at = Instant::now();
                    for (address, args) in decode(&buf[..n]) {
                        if tx.send(Message { address, args, at }).is_err() {
                            return;
                        }
                    }
                    ctx.request_repaint();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { port, rx, stop, last: None })
    }
}

/// This computer's address on the local network (what a phone sends to), when it has one.
fn local_ip() -> Option<String> {
    // connecting a UDP socket sends nothing; it only picks the interface a packet would leave by
    let s = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?;
    s.local_addr().ok().map(|a| a.ip().to_string()).filter(|ip| ip != "0.0.0.0")
}

/// A continuous address (its last value in a frame is all that matters).
fn continuous(address: &str) -> bool {
    address.starts_with("/velo/scene/macro/") || address.starts_with("/velo/param/") || address == "/velo/tempo/bpm"
}

impl App {
    /// Keeps the port in step with the preferences; applies what came since the last frame.
    pub fn poll_osc(&mut self, ctx: &egui::Context, now: f64) {
        let prefs = self.settings.osc.clone();
        if self.osc.as_ref().is_some_and(|o| !prefs.enabled || o.port != prefs.port) {
            self.osc = None;
        }
        if prefs.enabled && self.osc.is_none() {
            match Osc::start(prefs.port, ctx) {
                Ok(osc) => self.osc = Some(osc),
                Err(e) => {
                    self.settings.osc.enabled = false;
                    self.say(Tone::Error, tr!("OSC can't listen on port {}: {}", prefs.port, e));
                }
            }
        }
        let messages: Vec<Message> = self.osc.as_ref().map(|o| o.rx.try_iter().collect()).unwrap_or_default();
        // a fader's last position in the frame is all that matters; triggers all count
        let keep: Vec<bool> = messages.iter().enumerate().map(|(i, m)| !continuous(&m.address) || !messages[i + 1..].iter().any(|n| n.address == m.address)).collect();
        let mut last = None;
        for (m, keep) in messages.into_iter().zip(keep) {
            if keep {
                let known = self.apply_osc(&m, now);
                last = Some((m.address, m.at, known));
            }
        }
        if let (Some(last), Some(osc)) = (last, &mut self.osc) {
            osc.last = Some(last);
        }
        self.send_osc(ctx, now);
    }

    /// OSC out (docs/control.md §6): the values the preferences list to their host and port,
    /// each as it changes, or all of them at a fixed rate.
    fn send_osc(&mut self, ctx: &egui::Context, now: f64) {
        let p = &self.settings.osc;
        let to = (p.send_host.trim().to_string(), p.send_port);
        if !p.send || self.osc_out.tx.is_some() && self.osc_out.to != to {
            self.osc_out = Out::default();
        }
        if !self.settings.osc.send {
            return;
        }
        if self.osc_out.tx.is_none() {
            match Out::start(&to.0, to.1) {
                Ok(tx) => (self.osc_out.tx, self.osc_out.to) = (Some(tx), to.clone()),
                Err(e) => {
                    self.settings.osc.send = false;
                    return self.say(Tone::Error, tr!("OSC can't send to {}: {}", format!("{}:{}", to.0, to.1), e));
                }
            }
        }
        // at a fixed rate everything goes out on each tick (and the UI wakes for it); else what changed
        let rate = self.settings.osc.send_rate;
        let tick = rate > 0 && now >= self.osc_out.next;
        if rate > 0 {
            if tick {
                self.osc_out.next = now + 1.0 / f64::from(rate);
            }
            ctx.request_repaint_after(Duration::from_secs_f64((self.osc_out.next - now).max(0.0)));
        }
        let values = self.settings.osc.send_values.clone();
        let mut packets = Vec::new();
        for address in values.lines().map(str::trim).filter(|a| a.starts_with('/')) {
            let Some(arg) = self.osc_value(address) else { continue };
            let changed = self.osc_out.sent.get(address) != Some(&arg);
            if if rate > 0 { tick } else { changed } {
                packets.push(message(address, std::slice::from_ref(&arg)));
                self.osc_out.sent.insert(address.to_string(), arg);
            }
        }
        // the thread ends when the host has no address
        if let Some(tx) = &self.osc_out.tx
            && packets.into_iter().any(|p| tx.send(p).is_err())
        {
            self.settings.osc.send = false;
            self.say(Tone::Error, tr!("OSC can't find {}", to.0));
        }
    }

    /// What an address of OSC out reads now: the scene on the output (its name), the tempo, the
    /// beat (counted), a macro of the scene on the output, a parameter along its range, a signal
    /// node's output (of the shared signals or the scene being edited). None: nothing there.
    fn osc_value(&self, address: &str) -> Option<Arg> {
        let parts: Vec<&str> = address.trim_matches('/').split('/').collect();
        let live = self.live.as_ref();
        let level = |k: f32| Arg::Float(f64::from(k));
        Some(match parts.as_slice() {
            ["velo", "scene", "live"] => Arg::Str(self.session.project.scene(&self.stage_scene()?)?.name.clone()),
            ["velo", "tempo", "bpm"] => level(live?.clock()[0]),
            ["velo", "tempo", "beat"] => Arg::Int(live?.beats().floor() as i64),
            ["velo", "scene", "macro", n] => level(self.travel(&Target::Macro(n.parse::<u8>().ok()?.checked_sub(1)?))?),
            ["velo", "param", node, param] => level(self.travel(&Target::Param { node: self.node_id(node)?, param: param.to_string() })?),
            ["velo", "signal", node, port @ ..] => level(live?.signal(&self.node_id(node)?, port.first().map_or(Ok(0), |p| p.parse()).ok()?)?),
            _ => return None,
        })
    }

    /// A node by its id, or by its name with spaces as underscores, any case.
    fn node_id(&self, node: &str) -> Option<String> {
        let wanted = node.replace('_', " ");
        self.session.project.graphs().flat_map(|s| &s.nodes).find(|n| n.id == node || n.name.eq_ignore_ascii_case(&wanted)).map(|n| n.id.clone())
    }

    /// One message: true when the address means something to Velo.
    pub(crate) fn apply_osc(&mut self, m: &Message, now: f64) -> bool {
        let parts: Vec<&str> = m.address.trim_matches('/').split('/').collect();
        let number = m.args.first().and_then(Arg::number);
        let text = m.args.first().and_then(|a| if let Arg::Str(s) = a { Some(s.clone()) } else { None });
        // a scene by number (1…) or by name
        let scene = |app: &App| -> Option<usize> {
            let scenes = &app.session.project.scenes;
            match (&text, number) {
                (Some(name), _) if number.is_none() => scenes.iter().position(|s| s.name.eq_ignore_ascii_case(name.trim())),
                (_, Some(n)) if n >= 1.0 => Some(n as usize - 1).filter(|&i| i < scenes.len()),
                _ => None,
            }
        };
        match parts.as_slice() {
            ["velo", "scene", "macro", n] => {
                let (Ok(n), Some(k)) = (n.parse::<u8>(), number) else { return false };
                if !(1..=8).contains(&n) {
                    return false;
                }
                self.set_travel(&Target::Macro(n - 1), k.clamp(0.0, 1.0) as f32, now);
            }
            ["velo", "scene", "launch"] => match scene(self) {
                Some(i) => self.trigger(&Target::LaunchScene(i), m.at, now),
                None => return false,
            },
            ["velo", "scene", "select"] => match scene(self) {
                Some(i) => self.session.active_scene = Some(self.session.project.scenes[i].id.clone()),
                None => return false,
            },
            ["velo", "variation", n] => match n.parse::<usize>() {
                Ok(n) if n >= 1 => self.trigger(&Target::Variation(n - 1), m.at, now),
                _ => return false,
            },
            ["velo", "param", node, param] => {
                let Some(k) = number else { return false };
                let Some(id) = self.node_id(node) else { return false };
                let target = Target::Param { node: id, param: param.to_string() };
                if self.travel(&target).is_none() {
                    return false;
                }
                self.set_travel(&target, k.clamp(0.0, 1.0) as f32, now);
            }
            ["velo", "tempo", "tap"] => self.trigger(&Target::TapTempo, m.at, now),
            ["velo", "tempo", "bpm"] => match (number, &self.live) {
                (Some(bpm), Some(live)) => {
                    live.set_bpm(bpm as f32);
                    self.settings.auto_bpm = false;
                }
                _ => return false,
            },
            ["velo", "tempo", "resync"] => match &self.live {
                Some(live) => {
                    live.resync_at(m.at);
                    self.settings.auto_bpm = false;
                }
                None => return false,
            },
            ["velo", "blackout"] => match number {
                Some(k) => self.set_travel(&Target::Blackout, if k >= 0.5 { 1.0 } else { 0.0 }, now),
                None => self.trigger(&Target::Blackout, m.at, now),
            },
            ["velo", "freeze"] => match number {
                Some(k) => self.set_travel(&Target::Freeze, if k >= 0.5 { 1.0 } else { 0.0 }, now),
                None => self.trigger(&Target::Freeze, m.at, now),
            },
            ["velo", "op", id] => {
                if !self.settings.osc.ops {
                    return false;
                }
                let props = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(json!({}));
                if velo_core::OPERATORS.iter().all(|o| o.id != *id) {
                    return false;
                }
                self.run(id, props);
            }
            _ => return self.osc_bound(m, number, now),
        }
        true
    }

    /// An address of the sender's own ("/1/fader1", docs/control.md §6): learned for the control
    /// that waits, as MIDI is, or what its bindings do. True when it meant something.
    fn osc_bound(&mut self, m: &Message, number: Option<f64>, now: f64) -> bool {
        // no argument: a press
        let k = number.map_or(1.0, |n| n.clamp(0.0, 1.0) as f32);
        let prev = self.osc_heard.insert(m.address.clone(), k);
        if let Some(target) = self.learn.target.clone()
            && !m.address.starts_with("/velo/")
        {
            // the first that means something: a button pressed, a fader moved two steps
            let pressed = k >= 0.5 && prev.is_none_or(|p| p < 0.5);
            if pressed || prev.is_some_and(|p| (k - p).abs() >= 2.0 / 127.0) {
                // a button sends its ends, a fader what lies between
                let behavior = if k == 0.0 || k == 1.0 { midi::press_behavior(&target) } else { midi::behavior_for(&target, MidiMsg::Cc(0)) };
                self.bind(Binding { control: Control::Osc(m.address.clone()), target, behavior });
            }
            return true;
        }
        let bound: Vec<Binding> = self.session.project.bindings.iter().filter(|b| matches!(&b.control, Control::Osc(a) if *a == m.address)).cloned().collect();
        for b in &bound {
            let held = |v: f32| v >= 0.5;
            match (b.behavior, self.travel(&b.target)) {
                (Behavior::Absolute { takeover }, Some(current)) => {
                    if takeover == Takeover::Jump || midi::picks_up(prev, k, current) {
                        self.set_travel(&b.target, k, now);
                    }
                }
                (Behavior::Momentary, _) => {
                    if prev.map(held) != Some(held(k)) {
                        self.set_travel(&b.target, f32::from(u8::from(held(k))), now);
                    }
                }
                // a press: rising past a half
                _ => {
                    if held(k) && !prev.is_some_and(held) {
                        self.trigger(&b.target, m.at, now);
                    }
                }
            }
        }
        !bound.is_empty()
    }

    /// Preferences → OSC.
    pub fn prefs_osc(&mut self, ui: &mut Ui, t: &Theme) {
        kit::page_title(ui, t, icons::BROADCAST, tr!("Control"), tr!("Drive Velo over OSC from a phone, a lighting desk or another app."));
        kit::card(ui, t, tr!("OSC"), |ui| {
            let o = &mut self.settings.osc;
            kit::setting(ui, t, tr!("Listen for OSC"), tr!("From TouchOSC, a lighting desk or any tool that sends OSC."), |ui| kit::switch(ui, t, &mut o.enabled, tr!("Listen for OSC")));
            ui.add_enabled_ui(o.enabled, |ui| {
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("Port"), "", |ui| ui.add(egui::DragValue::new(&mut o.port).range(1024..=65535)));
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("Run any command"), tr!("/velo/op can change or delete anything in the project, from anyone on this network."), |ui| kit::switch(ui, t, &mut o.ops, tr!("Run any command")))
            });
            let status = match (&self.osc, local_ip()) {
                (Some(osc), ip) => {
                    let to = ip.map_or_else(|| format!("port {}", osc.port), |ip| format!("{ip}:{}", osc.port));
                    match &osc.last {
                        Some((address, at, known)) => tr!("Send to {} · last: {} {} s ago{}", to, address, at.elapsed().as_secs(), if *known { "" } else { " (unknown)" }),
                        None => tr!("Send to {} · nothing yet", to),
                    }
                }
                (None, _) => tr!("Off").to_string(),
            };
            kit::callout(ui, t, icons::BROADCAST, &status, if self.osc.is_some() { t.accent } else { t.fg_muted });
            ui.add_space(t.pad);
        });
        kit::card(ui, t, tr!("SEND OSC"), |ui| {
            let o = &mut self.settings.osc;
            kit::setting(ui, t, tr!("Send OSC"), tr!("The values below, to another app or a phone: a TouchOSC fader on /velo/scene/macro/1 follows the macro."), |ui| kit::switch(ui, t, &mut o.send, tr!("Send OSC")));
            ui.add_enabled_ui(o.send, |ui| {
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("To"), tr!("A computer's name or address, and its port."), |ui| {
                    ui.add(egui::DragValue::new(&mut o.send_port).range(1..=65535));
                    ui.add(kit::field(&mut o.send_host).desired_width(160.0));
                });
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("Times a second"), tr!("0: each value as it changes."), |ui| ui.add(egui::DragValue::new(&mut o.send_rate).range(0..=120)));
                kit::divider(ui, t);
                kit::setting(ui, t, tr!("Values"), tr!("An address a line: /velo/scene/live, /velo/tempo/bpm, /velo/tempo/beat, /velo/scene/macro/1, /velo/param/<node>/<param>, /velo/signal/<node>"), |_| ());
                ui.add(egui::TextEdit::multiline(&mut o.send_values).desired_rows(4).desired_width(f32::INFINITY).font(Theme::mono(t.text_sm)));
            });
            ui.add_space(t.pad);
        });
        kit::card(ui, t, "MIDI", |ui| {
            let detail = tr!("Each controller's lights follow Velo, through its output of the same name: a toggled pad, the playing scene's pad, a knob's value. Set its pads to momentary: Velo keeps the toggle.");
            kit::setting(ui, t, tr!("Light the controllers"), detail, |ui| kit::switch(ui, t, &mut self.settings.midi_feedback, tr!("Light the controllers")));
            ui.add_space(t.pad);
        });
        kit::card(ui, t, tr!("ADDRESSES"), |ui| {
            let addresses = [
                ("/velo/scene/macro/1 f", tr!("Macro 1 to 8 of the selected scene, 0 to 1")),
                ("/velo/scene/launch i|s", tr!("Launches a scene, by number or name")),
                ("/velo/scene/select i|s", tr!("Selects a scene, by number or name")),
                ("/velo/variation/1", tr!("Recalls a variation")),
                ("/velo/param/<node>/<param> f", tr!("A parameter, 0 to 1 across its range")),
                ("/velo/tempo/tap", tr!("Taps the tempo")),
                ("/velo/tempo/bpm f", tr!("Sets the tempo")),
                ("/velo/tempo/resync", tr!("The downbeat is now")),
                ("/velo/blackout [f]", tr!("Flips blackout, or 1 on and 0 off")),
                ("/velo/freeze [f]", tr!("Flips freeze, or 1 on and 0 off")),
                ("/velo/op/<operator> s", tr!("Runs an operator, its settings as JSON")),
                ("/any/address f", tr!("Any other: learned on a control, as MIDI is")),
            ];
            egui::Grid::new("osc addresses").num_columns(2).spacing([t.pad * 2.0, t.gap + 2.0]).show(ui, |ui| {
                for (address, what) in addresses {
                    ui.add(egui::Label::new(RichText::new(address).font(Theme::mono(t.text_xs)).color(t.fg_text)).wrap_mode(egui::TextWrapMode::Extend));
                    ui.label(RichText::new(what).small().color(t.fg_muted));
                    ui.end_row();
                }
            });
            ui.add_space(t.pad);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (One test, as OSC out's port is the process's: two at once would cross.)
    #[test]
    fn osc_out_sends_the_chosen_values_as_they_change_or_at_a_rate_and_never_to_itself() {
        let ctx = egui::Context::default();
        let mut app = App::for_tests();
        app.run("scene.add", json!({}));
        let listen = UdpSocket::bind("127.0.0.1:0").unwrap();
        listen.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let o = &mut app.settings.osc;
        (o.send, o.send_host, o.send_port) = (true, "localhost".into(), listen.local_addr().unwrap().port());
        o.send_values = "/velo/scene/macro/1\n/velo/scene/live\n/velo/nothing\n/velo/tempo/bpm".into();
        let heard = || -> Vec<(String, Vec<Arg>)> {
            let mut buf = [0; 1024];
            std::iter::from_fn(|| listen.recv(&mut buf).ok().map(|n| decode(&buf[..n]))).flatten().collect()
        };
        let macro1 = |v: f64| ("/velo/scene/macro/1".to_string(), vec![Arg::Float(v)]);
        // what has a value, once (no engine here: no tempo); names are looked up on OSC out's thread
        app.poll_osc(&ctx, 1.0);
        assert_eq!(heard(), [macro1(0.0), ("/velo/scene/live".to_string(), vec![Arg::Str("Scene 1".into())])]);
        app.poll_osc(&ctx, 1.1);
        assert!(heard().is_empty(), "nothing changed");
        app.run("macro.set", json!({ "index": 0, "value": 0.5 }));
        app.poll_osc(&ctx, 1.2);
        assert_eq!(heard(), [macro1(0.5)], "only what changed");
        // ten times a second: everything on each tick, changed or not
        app.settings.osc.send_rate = 10;
        app.poll_osc(&ctx, 2.0);
        assert_eq!(heard().len(), 2);
        app.poll_osc(&ctx, 2.05);
        assert!(heard().is_empty(), "between ticks");
        app.poll_osc(&ctx, 2.11);
        assert_eq!(heard().len(), 2);
        // pointed at Velo's own OSC in: what it sends there is dropped, anyone else's comes in
        let port = UdpSocket::bind("127.0.0.1:0").and_then(|s| s.local_addr()).unwrap().port();
        let osc = Osc::start(port, &ctx).unwrap();
        app.settings.osc.send_port = port;
        app.poll_osc(&ctx, 3.0);
        assert!(osc.rx.recv_timeout(Duration::from_millis(400)).is_err(), "Velo's own, dropped");
        UdpSocket::bind("127.0.0.1:0").unwrap().send_to(&message("/velo/tempo/tap", &[]), ("127.0.0.1", port)).unwrap();
        assert_eq!(osc.rx.recv_timeout(Duration::from_secs(2)).unwrap().address, "/velo/tempo/tap");
    }

    #[test]
    fn messages_and_bundles_decode() {
        let one = message("/velo/scene/macro/2", &[Arg::Float(0.5)]);
        assert_eq!(decode(&one), [("/velo/scene/macro/2".to_string(), vec![Arg::Float(0.5)])]);
        let two = message("/velo/scene/launch", &[Arg::Str("Drop".into()), Arg::Int(3), Arg::Bool(true)]);
        assert_eq!(decode(&two)[0].1, [Arg::Str("Drop".into()), Arg::Int(3), Arg::Bool(true)]);
        // a bundle of both
        let mut bundle = b"#bundle\0".to_vec();
        bundle.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
        for m in [&one, &two] {
            bundle.extend_from_slice(&(m.len() as u32).to_be_bytes());
            bundle.extend_from_slice(m);
        }
        assert_eq!(decode(&bundle).len(), 2);
        // no type tags (old senders), cut short, not OSC
        assert_eq!(decode(&message("/velo/tempo/tap", &[])), [("/velo/tempo/tap".to_string(), vec![])]);
        assert!(decode(&one[..one.len() - 2]).is_empty());
        assert!(decode(b"hello").is_empty());
    }
}
