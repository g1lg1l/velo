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
//! /velo/op/<operator>            [s]      any operator, its props as JSON
//! ```

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use velo_core::Target;

use crate::shell::Tone;
use crate::theme::Theme;
use crate::{App, icons, kit};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub enabled: bool,
    pub port: u16,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { enabled: false, port: 9000 }
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
                    let Ok((n, _)) = socket.recv_from(&mut buf) else { continue };
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
        let Some(osc) = &self.osc else { return };
        let messages: Vec<Message> = osc.rx.try_iter().collect();
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
                // a node by its id, or its name with spaces as underscores, any case
                let wanted = node.replace('_', " ");
                let id = self.session.project.scenes.iter().flat_map(|s| &s.nodes).find(|n| n.id == *node || n.name.eq_ignore_ascii_case(&wanted)).map(|n| n.id.clone());
                let Some(id) = id else { return false };
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
                let props = text.and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(json!({}));
                if velo_core::OPERATORS.iter().all(|o| o.id != *id) {
                    return false;
                }
                self.run(id, props);
            }
            _ => return false,
        }
        true
    }

    /// Preferences → OSC.
    pub fn prefs_osc(&mut self, ui: &mut Ui, t: &Theme) {
        kit::section(ui, t, icons::BROADCAST, tr!("OSC"), true, |ui| {
            let o = &mut self.settings.osc;
            kit::setting(ui, t, tr!("Listen for OSC"), tr!("From TouchOSC, a lighting desk or any tool that sends OSC."), |ui| kit::switch(ui, t, &mut o.enabled, tr!("Listen for OSC")));
            ui.add_enabled_ui(o.enabled, |ui| kit::setting(ui, t, tr!("Port"), "", |ui| ui.add(egui::DragValue::new(&mut o.port).range(1024..=65535))));
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
            kit::note(ui, t, Some(icons::BROADCAST), &status, if self.osc.is_some() { t.accent } else { t.fg_muted });
        });
        kit::section(ui, t, icons::LIST_BULLETS, tr!("ADDRESSES"), false, |ui| {
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
            ];
            egui::Grid::new("osc addresses").num_columns(2).spacing([t.pad * 2.0, t.gap + 2.0]).show(ui, |ui| {
                for (address, what) in addresses {
                    ui.add(egui::Label::new(RichText::new(address).font(Theme::mono(t.text_xs)).color(t.fg_text)).wrap_mode(egui::TextWrapMode::Extend));
                    ui.label(RichText::new(what).small().color(t.fg_muted));
                    ui.end_row();
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An OSC message as TouchOSC sends it.
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
