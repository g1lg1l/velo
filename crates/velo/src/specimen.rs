//! The specimen board (spike S2, ARCHITECTURE.md §4): every widget in its states, moved by fake
//! signals at 128 BPM, on top of the real UI. Opened from the command palette ("Specimen
//! board", or `VELO_SPECIMEN=1` at start). It also shows what the UI costs per frame, with a
//! stress mode of 200 modulated sliders.

use std::f32::consts::TAU;

use eframe::egui::{self, Id, RichText, Ui, vec2};
use velo_core::{Curve, ParamInfo};

use crate::icons;
use crate::kit::{self, Variant};
use crate::theme::Theme;
use crate::widgets::{self, CardState, ModView};

const SPEED: ParamInfo = ParamInfo { name: "Speed", min: 0.0, max: 2.0, default: 0.5, curve: Curve::Linear, unit: "" };
const TWIST: ParamInfo = ParamInfo { name: "Twist", min: -1.0, max: 1.0, default: 0.0, curve: Curve::Linear, unit: "" };
const ZOOM: ParamInfo = ParamInfo { name: "Zoom", min: 0.25, max: 4.0, default: 1.0, curve: Curve::Log, unit: "" };
const AMOUNT: ParamInfo = ParamInfo { name: "Amount", min: -1.0, max: 1.0, default: 0.5, curve: Curve::Linear, unit: "%" };
const ATTACK: ParamInfo = ParamInfo { name: "Attack", min: 1.0, max: 500.0, default: 5.0, curve: Curve::Log, unit: "ms" };
const RELEASE: ParamInfo = ParamInfo { name: "Release", min: 1.0, max: 2000.0, default: 150.0, curve: Curve::Log, unit: "ms" };
const INVERT: ParamInfo = ParamInfo { name: "Invert", min: 0.0, max: 1.0, default: 0.0, curve: Curve::Steps, unit: "onoff" };

/// Fake audio features: a four-on-the-floor loop at 128 BPM.
struct Signals {
    level: f32,
    bass: f32,
    mid: f32,
    high: f32,
    kick: f32,
    snare: f32,
    hat: f32,
    lfo: f32,
    /// 0..1 through the bar.
    bar: f32,
}

impl Signals {
    fn at(time: f64) -> Self {
        let beat = time * 128.0 / 60.0;
        let since = |every: f64, offset: f64| (((beat - offset).rem_euclid(every)) * 60.0 / 128.0) as f32;
        let hit = |since: f32, decay: f32| (-since / decay).exp();
        let kick = hit(since(1.0, 0.0), 0.12);
        let snare = hit(since(2.0, 1.0), 0.16);
        let hat = hit(since(0.5, 0.25), 0.05);
        let slow = (time as f32 * 0.7).sin() * 0.5 + 0.5;
        Self {
            level: 0.45 + 0.35 * kick + 0.1 * slow,
            bass: 0.35 + 0.6 * kick,
            mid: 0.3 + 0.4 * snare + 0.1 * slow,
            high: 0.2 + 0.6 * hat,
            kick,
            snare,
            hat,
            lfo: (time as f32 * TAU / (4.0 * 60.0 / 128.0)).sin() * 0.5 + 0.5,
            bar: (beat.rem_euclid(4.0) / 4.0) as f32,
        }
    }

    fn get(&self, name: &str) -> f32 {
        match name {
            "Level" => self.level,
            "Bass" => self.bass,
            "Mid" => self.mid,
            "High" => self.high,
            "Kick" => self.kick,
            "Snare" => self.snare,
            "Hat" => self.hat,
            _ => self.lfo,
        }
    }
}

/// A mapping on the board's Speed slider.
struct Mapping {
    source: String,
    amount: f32,
    curve: usize,
    attack: f32,
    release: f32,
    invert: bool,
}

impl Mapping {
    fn new(source: &str) -> Self {
        Self { source: source.into(), amount: 0.5, curve: 0, attack: ATTACK.default, release: RELEASE.default, invert: false }
    }
}

pub struct Specimen {
    speed: f32,
    twist: f32,
    zoom: f32,
    mappings: Vec<Mapping>,
    view: usize,
    quantize: usize,
    stress: bool,
}

impl Default for Specimen {
    fn default() -> Self {
        Self { speed: 0.6, twist: -0.2, zoom: 1.0, mappings: vec![Mapping::new("Kick")], view: 0, quantize: 1, stress: false }
    }
}

impl Specimen {
    /// `VELO_SPECIMEN=1` opens the board at start, `VELO_SPECIMEN=stress` with the stress test on.
    pub fn from_env() -> Option<Self> {
        let v = std::env::var("VELO_SPECIMEN").ok()?;
        Some(Self { stress: v == "stress", ..Default::default() })
    }

    /// Draws the board; returns false once it's closed. `cpu` is the UI's frame cost (average
    /// and worst over the last two seconds, in seconds).
    pub fn show(&mut self, ctx: &egui::Context, t: &Theme, cpu: (f32, f32), layout: (f32, f32)) -> bool {
        let mut open = true;
        let s = Signals::at(ctx.input(|i| i.time));
        egui::Window::new("specimen").title_bar(false).frame(kit::window_frame(ctx, t)).default_size([760.0, 720.0]).resizable(true).show(ctx, |ui| {
            kit::region(ui);
            kit::bar(ui, t, |ui| {
                ui.label(RichText::new(tr!("Specimen board")).strong().color(t.fg_text));
                ui.label(RichText::new(tr!("every widget, moved by a fake 128 BPM loop")).color(t.fg_muted));
                kit::right(ui, |ui| {
                    if kit::icon_button(ui, t, icons::X, tr!("Close (Esc)"), false).clicked() {
                        open = false;
                    }
                });
            });
            kit::scroll(ui, t, "specimen", true, t.bg_base, |ui| self.board(ui, t, &s, cpu, layout));
        });
        // the fake signals move every frame
        ctx.request_repaint();
        open && !ctx.input(|i| i.key_pressed(egui::Key::Escape))
    }

    fn board(&mut self, ui: &mut Ui, t: &Theme, s: &Signals, cpu: (f32, f32), layout: (f32, f32)) {
        ui.spacing_mut().item_spacing = vec2(t.gap * 2.0, t.gap * 2.0);
        kit::section(ui, t, icons::CURSOR_CLICK, tr!("BUTTONS"), true, |ui| {
            ui.horizontal(|ui| {
                kit::action(ui, t, Variant::Primary, Some(icons::PLAY), tr!("Go live"));
                kit::action(ui, t, Variant::Secondary, Some(icons::PLUS), tr!("Add layer"));
                kit::action(ui, t, Variant::Outline, Some(icons::METRONOME), tr!("Tap"));
                kit::action(ui, t, Variant::Ghost, None, tr!("Cancel"));
                kit::action(ui, t, Variant::Destructive, Some(icons::POWER), tr!("Blackout"));
            });
            ui.horizontal(|ui| {
                kit::icon_button(ui, t, icons::LINK, tr!("Link"), true);
                kit::icon_button(ui, t, icons::SLIDERS, tr!("Mappings"), false);
                kit::button(ui, t, Some(icons::LIGHTNING), tr!("Learn"), true);
                let items = [(0, icons::STACK, tr!("Layers")), (1, icons::TREE_STRUCTURE, tr!("Patch"))];
                if let Some(v) = kit::tabs(ui, t, Id::unique("specimen-tabs"), self.view, &items, false, |_| false) {
                    self.view = v;
                }
                if let Some(q) = kit::segmented(ui, t, self.quantize, &[tr!("Off"), tr!("Beat"), tr!("Bar"), tr!("4 bars")]) {
                    self.quantize = q;
                }
            });
        });

        kit::section(ui, t, icons::WAVEFORM, tr!("SOURCES · DRAG ONE ONTO A SLIDER"), false, |ui| {
            ui.horizontal_wrapped(|ui| {
                for name in ["Level", "Bass", "Mid", "High"] {
                    widgets::source_meter(ui, t, name, name, s.get(name), widgets::Meter::Level, 72.0);
                }
                for name in ["Kick", "Snare", "Hat"] {
                    widgets::source_meter(ui, t, name, name, s.get(name), widgets::Meter::Hit(widgets::TE_ORANGE), 72.0);
                }
                widgets::source_meter(ui, t, "LFO 1", "LFO 1", s.lfo, widgets::Meter::Level, 72.0);
            });
        });

        kit::section(ui, t, icons::SLIDERS_HORIZONTAL, tr!("PARAMETERS"), false, |ui| {
            ui.columns(2, |cols| {
                self.sliders(&mut cols[0], t, s);
                let ui = &mut cols[1];
                ui.horizontal(|ui| {
                    for (info, value) in [(&SPEED, &mut self.speed), (&ZOOM, &mut self.zoom)] {
                        if let (_, Some(v)) = widgets::knob(ui, t, info, *value) {
                            *value = v;
                        }
                    }
                    widgets::switch(ui, t, &INVERT, 1.0);
                });
                ui.label(RichText::new(tr!("Knobs and switches from Ravune, for comparison. Alt-drag a slider to change its first mapping; click a badge for its settings.")).small().color(t.fg_muted));
            });
        });

        kit::section(ui, t, icons::STACK, tr!("SCENES"), false, |ui| {
            ui.columns(2, |cols| {
                let cards = [
                    ("Intro", CardState { selected: true, ..Default::default() }),
                    ("Build", CardState { armed: Some(s.bar), ..Default::default() }),
                    ("Drop", CardState { live: Some(s.kick), ..Default::default() }),
                    ("Ambient", CardState::default()),
                ];
                for (i, (name, state)) in cards.into_iter().enumerate() {
                    widgets::scene_card(&mut cols[i % 2], t, i, name, "", i as f32 * 0.21 + 0.5, state);
                }
            });
        });

        kit::section(ui, t, icons::TEXT_AA, tr!("TYPE"), false, |ui| {
            ui.label(RichText::new(tr!("Velo makes sound visible")).size(t.text_lg).strong().color(t.fg_text));
            ui.label(RichText::new(tr!("Body text: pick an audio input, add a visual, drag Kick onto a parameter.")).color(t.fg_text));
            ui.label(RichText::new(tr!("Muted: hints, empty states, secondary labels.")).small().color(t.fg_muted));
            ui.label(RichText::new("128.00 BPM · 60 fps · 1920×1080 · −12.4 dB").font(t.readout()).color(t.fg_text));
        });

        kit::section(ui, t, icons::SPEEDOMETER, tr!("PERFORMANCE"), false, |ui| {
            ui.horizontal(|ui| {
                let ms = |s: f32| format!("{:.2}", s * 1000.0);
                ui.label(RichText::new(tr!("Frame {} ms avg / {} worst · layout {} / {}", ms(cpu.0), ms(cpu.1), ms(layout.0), ms(layout.1))).font(t.readout()).color(t.fg_text));
                if kit::button(ui, t, Some(icons::SLIDERS), tr!("Stress: 200 sliders"), self.stress).clicked() {
                    self.stress = !self.stress;
                }
            });
            if self.stress {
                ui.columns(4, |cols| {
                    for i in 0..200 {
                        let phase = i as f32 * 0.37;
                        let level = ((s.lfo * TAU + phase).sin() * 0.5 + 0.5) * s.kick.max(0.3);
                        let mods = [ModView { source: "LFO 1", amount: 0.4, level }];
                        widgets::slider(&mut cols[i % 4], t, &SPEED, 0.5 + 0.4 * (phase.sin()), &widgets::Marks { mods: &mods, ..Default::default() });
                    }
                });
            }
        });
    }

    /// Speed with its mappings and their popovers, Twist with a learned knob, Zoom bare.
    fn sliders(&mut self, ui: &mut Ui, t: &Theme, s: &Signals) {
        let views: Vec<ModView> = self.mappings.iter().map(|m| ModView { source: &m.source, amount: m.amount, level: s.get(&m.source) }).collect();
        let out = widgets::slider(ui, t, &SPEED, self.speed, &widgets::Marks { mods: &views, ..Default::default() });
        if let Some(v) = out.value {
            self.speed = v;
        }
        if let (Some(a), Some(first)) = (out.amount, self.mappings.first_mut()) {
            first.amount = a;
        }
        if let Some(source) = out.dropped
            && !self.mappings.iter().any(|m| m.source == source)
        {
            self.mappings.push(Mapping::new(&source));
        }
        let mut remove = None;
        for (i, badge) in out.badges.iter().enumerate() {
            let popup = egui::Popup::from_toggle_button_response(badge).frame(kit::popover_frame(ui.ctx(), t)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(260.0);
            popup.show(|ui| {
                if mapping_popover(ui, t, &mut self.mappings[i]) {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.mappings.remove(i);
        }
        let out = widgets::slider(ui, t, &TWIST, self.twist, &widgets::Marks { binding: Some("CC 21"), ..Default::default() });
        if let Some(v) = out.value {
            self.twist = v;
        }
        let out = widgets::slider(ui, t, &ZOOM, self.zoom, &widgets::Marks::default());
        if let Some(v) = out.value {
            self.zoom = v;
        }
    }
}

/// A mapping's settings (product.md §7.3). Returns true when it should be removed.
fn mapping_popover(ui: &mut Ui, t: &Theme, m: &mut Mapping) -> bool {
    ui.spacing_mut().item_spacing = vec2(t.gap * 2.0, t.gap * 2.0);
    ui.label(RichText::new(tr!("{} → Speed", m.source)).strong().color(t.fg_text));
    if let Some(v) = widgets::slider(ui, t, &AMOUNT, m.amount, &widgets::Marks::default()).value {
        m.amount = v;
    }
    ui.label(RichText::new(tr!("Curve")).small().color(t.fg_muted));
    if let Some(c) = kit::segmented(ui, t, m.curve, &[tr!("Linear"), tr!("In"), tr!("Out"), tr!("S")]) {
        m.curve = c;
    }
    ui.horizontal(|ui| {
        for (info, value) in [(&ATTACK, &mut m.attack), (&RELEASE, &mut m.release)] {
            if let (_, Some(v)) = widgets::knob(ui, t, info, *value) {
                *value = v;
            }
        }
        if let (_, Some(v)) = widgets::switch(ui, t, &INVERT, if m.invert { 1.0 } else { 0.0 }) {
            m.invert = v >= 0.5;
        }
    });
    kit::action(ui, t, Variant::Ghost, Some(icons::TRASH), tr!("Remove mapping")).clicked()
}
