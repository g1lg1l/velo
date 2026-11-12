//! The window around the editors: top bar, status bar, toasts, Preferences, the shortcut
//! sheet, the command palette and the rename dialog.
//! From Ravune (bd10814), trimmed for Velo.

use std::f32::consts::{FRAC_PI_2, PI};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, CornerRadius, FontId, Id, Key, Modifiers, Popup, RichText, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;
use velo_core::{Control, OPERATORS};

use std::sync::atomic::Ordering::Relaxed;

use crate::host::{Request, Target};
use crate::i18n::{self, Lang};
use crate::keymap::{self, COMMANDS};
use crate::live::{AudioState, Hearing, Input};
use crate::theme::{THEME_EXTENSION, Theme};
use crate::{App, Panels, icons, kit, midi, widgets};
use velo_engine::share::Share;

/// What a toast is about: its badge, its title and their color (Sileo's states).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tone {
    Done,
    /// Velo waits on the user: a controller to touch, an update to install.
    Action,
    Error,
}

/// The one toast on screen; a new one replaces it.
pub struct Toast {
    tone: Tone,
    text: String,
    born: Instant,
    /// The message's spring: opening or closing, since when, from how open.
    open: bool,
    turned: Instant,
    from: f32,
    /// A button on the message ("Undo") and what it's for; what was asked, once clicked.
    action: Option<(String, &'static str)>,
    pub acted: Option<&'static str>,
    /// Seconds on screen.
    life: f32,
}

impl Toast {
    pub fn new(tone: Tone, text: String) -> Self {
        let now = Instant::now();
        Self { tone, text, born: now, open: false, turned: now, from: 0.0, action: None, acted: None, life: LIFE }
    }

    /// On screen for `secs` instead of the usual six (a message to read, not a notice).
    pub fn lasting(mut self, secs: f32) -> Self {
        self.life = secs;
        self
    }

    /// With a button on the message: when it's clicked, [`Self::acted`] says `what`.
    pub fn with_action(mut self, label: impl Into<String>, what: &'static str) -> Self {
        self.action = Some((label.into(), what));
        self
    }

    /// 0 folded into the pill, 1 out (a little over while the spring bounces).
    fn openness(&self, now: Instant) -> f32 {
        let k = (now - self.turned).as_secs_f32() / SPRING;
        let (to, curve) = if self.open { (1.0, kit::spring(k)) } else { (0.0, kit::ease(k)) };
        self.from + (to - self.from) * curve
    }
}

// Sileo's timing: 6 s on screen, the message drops out of the pill after 150 ms and folds back
// 2 s before the end; 600-ms springs with a 0.25 bounce, in and out in 0.4 s.
const LIFE: f32 = 6.0;
const EXPAND_AT: f32 = 0.15;
const COLLAPSE_AT: f32 = 4.0;
const SPRING: f32 = 0.6;
const ENTER: f32 = 0.4;
// and its geometry: a 28-px pill on a 350-px blob, 14-px corners, the message 16 px in from
// the sides, 14 under the pill and 16 over the bottom, its lines 19 px apart
const PILL_H: f32 = 28.0;
const WIDTH: f32 = 350.0;
const ROUND: f32 = 14.0;
const PAD: f32 = 16.0;
const PAD_TOP: f32 = 14.0;
const LINE_H: f32 = 19.0;
/// The inside corner where the gooey filter joins pill and body.
const FILLET: f32 = 10.0;
/// Where the toast sits, 0 left · 0.5 centre · 1 right: its header goes on the same side.
const SIDE: f32 = 0.5;

/// Auto-BPM's memory between frames.
#[derive(Default)]
pub struct Auto {
    /// The estimate last looked at (its beat), the tempo it said, when it last set the clock.
    taken: Option<std::time::Instant>,
    before: Option<f32>,
    set: Option<std::time::Instant>,
}

impl Auto {
    /// How sure an estimate must be to move the beat clock (a four-on-the-floor loop is ~0.7,
    /// noise ~0.1).
    pub const SURE: f32 = 0.3;
}

/// The rename dialog: which scene (and node or macro, when it's one), the text so far.
pub struct Renaming {
    pub scene: String,
    pub node: Option<String>,
    pub macro_index: Option<usize>,
    pub text: String,
    focused: bool,
}

impl Renaming {
    pub fn new(scene: impl Into<String>, text: impl Into<String>) -> Self {
        Self { scene: scene.into(), node: None, macro_index: None, text: text.into(), focused: false }
    }

    pub fn node(scene: &str, node: &str, text: &str) -> Self {
        Self { node: Some(node.into()), ..Self::new(scene, text) }
    }

    pub fn macro_(scene: &str, index: usize, text: &str) -> Self {
        Self { macro_index: Some(index), ..Self::new(scene, text) }
    }
}

/// A theme in the Preferences list: its name and a strip of its colors.
/// Where the picture shows (the top bar's preview menu).
#[derive(Clone, Copy, PartialEq)]
enum Preview {
    Window,
    Behind,
    Beside,
    Under,
    Off,
}

/// A folder as it's said: the home folder as ~.
fn tilde(dir: &std::path::Path) -> String {
    let text = dir.display().to_string();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => format!("~{}", &text[home.len()..]),
        _ => text,
    }
}

/// How long ago, in the largest unit that fits.
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => tr!("just now").to_string(),
        60..3600 => tr!("{} min ago", seconds / 60),
        3600..86_400 => tr!("{} h ago", seconds / 3600),
        86_400..172_800 => tr!("yesterday").to_string(),
        _ => tr!("{} days ago", seconds / 86_400),
    }
}

fn theme_row(ui: &mut Ui, t: &Theme, theme: &Theme, current: bool) -> egui::Response {
    let r = kit::row(ui, t, Id::unique(("theme-row", &theme.name)), current);
    let mut painter = ui.painter_at(r.rect);
    painter.multiply_opacity(kit::row_alpha(ui, &r));
    painter.text(r.rect.left_center() + vec2(t.pad, 0.0), Align2::LEFT_CENTER, &theme.name, FontId::proportional(13.0), t.fg_text);
    let size = r.rect.height() - 12.0;
    for (i, color) in theme.swatches().into_iter().rev().enumerate() {
        let min = pos2(r.rect.right() - t.pad - (i + 1) as f32 * (size + 2.0), r.rect.top() + 6.0);
        let swatch = egui::Rect::from_min_size(min, vec2(size, size));
        painter.rect_filled(swatch, CornerRadius::same(t.radius_sm / 2), color);
        painter.rect_stroke(swatch, CornerRadius::same(t.radius_sm / 2), Stroke::new(1.0, t.line), egui::StrokeKind::Inside);
    }
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, current, &theme.name));
    r
}

/// A group on the shortcut sheet: its name, then each line's shortcuts and what they do.
type SheetGroup<'a> = (&'a str, Vec<(Vec<String>, &'a str)>);

fn kbd(ui: &mut Ui, t: &Theme, keys: &str) {
    for key in keys.split(' ') {
        let galley = ui.painter().layout_no_wrap(key.to_string(), Theme::mono(11.0), t.fg_text);
        let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(10.0, 6.0), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(t.radius_sm / 2), t.bg_control);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, t.bg_field));
        ui.painter().galley(rect.min + vec2(5.0, 3.0), galley, t.fg_text);
    }
}

impl App {
    // ---- top bar ------------------------------------------------------------

    /// The live state, as docs/product.md §7.1 has it: the file, undo, then the audio input,
    /// tempo, MIDI, output and blackout (placeholders until the engines exist); search and
    /// preferences on the right.
    pub fn top_bar(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        let top = ui.max_rect();
        // narrower windows fold the less used controls away: first the panel toggles, the
        // frame rate and the search's label, then the chips' words
        let compact = top.width() < 1380.0;
        let tight = top.width() < 1120.0;
        kit::bar(ui, &t, |ui| {
            // the project: its name, plainly, and its menu
            let name = format!("{}{}  {}", self.file_name(), if self.dirty() { " •" } else { "" }, icons::CARET_DOWN);
            let r = kit::ghost(ui, &t, None, &name).on_hover_text(tr!("The project: new, open, save, export"));
            kit::menu(&t, &r).show(|ui| self.file_menu(ui));
            ui.add_space(t.pad);
            // the sound: what's heard, and its tempo
            kit::group(ui, &t, |ui| {
                self.audio_chip(ui, &t, tight);
                kit::group_line(ui, &t);
                self.tempo_chip(ui, &t);
            });
            ui.add_space(t.pad);
            // the stage: MIDI, where the picture goes, Freeze and Blackout (in the Output menu when
            // the bar is narrow); words when there's room, icons when there's less
            kit::group(ui, &t, |ui| {
                self.midi_chip(ui, &t, compact);
                kit::group_line(ui, &t);
                self.output_chip(ui, &t, compact, tight);
                if !tight {
                    kit::group_line(ui, &t);
                    let frozen = self.live.as_ref().is_some_and(|l| l.frozen);
                    let tip = self.tip(tr!("Freeze: the output holds this frame while you change things"), "stage.freeze");
                    let r = ui.add_enabled_ui(self.live.is_some(), |ui| kit::icon_button(ui, &t, icons::SNOWFLAKE, &tip, frozen)).inner;
                    if !self.learnable(ui, &r, &velo_core::Target::Freeze) && r.clicked() {
                        self.command("stage.freeze");
                    }
                    let blackout = self.live.as_ref().is_some_and(|l| l.blackout);
                    let tip = self.tip(tr!("Blackout: the output goes black, the preview keeps running"), "stage.blackout");
                    let r = ui.add_enabled_ui(self.live.is_some(), |ui| kit::alarm_button(ui, &t, icons::POWER, &tip, blackout)).inner;
                    if !self.learnable(ui, &r, &velo_core::Target::Blackout) && r.clicked() {
                        self.command("stage.blackout");
                    }
                }
            });
            kit::right(ui, |ui| {
                let r = kit::icon_button(ui, &t, icons::GEAR_SIX, &self.tip(tr!("Preferences"), "app.preferences"), self.show_settings);
                if r.clicked() {
                    self.command("app.preferences");
                }
                kit::vsep(ui, &t);
                self.chat_toggle(ui, &t);
                // the panels around the canvas, each glyph turned to its edge; focus mode lights up
                let open = self.settings.panels;
                let toggles = [
                    ("app.focus", icons::CORNERS_OUT, 0.0, tr!("Focus: hide or bring back every panel"), open == Panels::NONE),
                    ("app.toggle_inspector", icons::SIDEBAR_SIMPLE, PI, tr!("Show or hide the inspector"), open.inspector),
                    ("app.toggle_sources", icons::SIDEBAR_SIMPLE, -FRAC_PI_2, tr!("Show or hide the macros and sources"), open.sources),
                    ("app.toggle_scenes", icons::SIDEBAR_SIMPLE, 0.0, tr!("Show or hide the scenes"), open.scenes),
                ];
                if compact {
                    // all of them, and the frame-time HUD, behind one button
                    let r = kit::icon_button(ui, &t, icons::LAYOUT, tr!("Panels and view"), false);
                    let mut pick = None;
                    kit::menu(&t, &r).show(|ui| {
                        for (id, _, _, label, on) in toggles.iter().rev() {
                            if ui.selectable_label(*on, *label).clicked() {
                                pick = Some(*id);
                            }
                        }
                        ui.separator();
                        self.preview_choices(ui);
                        ui.separator();
                        if ui.selectable_label(self.settings.hud, tr!("Frame-time HUD")).clicked() {
                            self.settings.hud ^= true;
                        }
                    });
                    if let Some(id) = pick {
                        self.command(id);
                    }
                } else {
                    for (id, icon, angle, label, on) in toggles {
                        let tip = self.tip(label, id);
                        let r = if id == "app.focus" { kit::icon_button(ui, &t, icon, &tip, on) } else { kit::panel_toggle(ui, &t, icon, angle, &tip, on) };
                        if r.clicked() {
                            self.command(id);
                        }
                    }
                    // where the picture shows: a menu of its places
                    let showing = self.preview_place() != Preview::Off;
                    let r = kit::icon_button(ui, &t, icons::MONITOR_PLAY, tr!("Preview: in a window, behind the canvas, beside or under the nodes, or off"), showing);
                    let hud = self.tip(tr!("Frame-time HUD"), "app.hud");
                    kit::menu(&t, &r).show(|ui| {
                        self.preview_choices(ui);
                        ui.separator();
                        if ui.selectable_label(self.settings.hud, hud).clicked() {
                            self.settings.hud ^= true;
                        }
                    });
                }
                kit::vsep(ui, &t);
                // undo and redo, with the other actions
                for (id, icon, label) in [("edit.redo", icons::ARROW_U_UP_RIGHT, self.session.redo_label()), ("edit.undo", icons::ARROW_U_UP_LEFT, self.session.undo_label())] {
                    let what = if id == "edit.undo" { tr!("Undo") } else { tr!("Redo") };
                    let tip = self.tip(&format!("{what} {}", label.map_or(tr!("(nothing yet)"), i18n::t)), id);
                    let r = ui.add_enabled_ui(label.is_some(), |ui| kit::icon_button(ui, &t, icon, &tip, false)).inner;
                    if r.clicked() {
                        self.run(id, json!({}));
                    }
                }
                kit::vsep(ui, &t);
                let keys = self.settings.keymap.keys("app.palette").first().map(keymap::text).unwrap_or_default();
                let search = if compact { kit::icon_button(ui, &t, icons::MAGNIFYING_GLASS, &tr!("Search commands   {}", keys), false) } else { kit::button(ui, &t, Some(icons::MAGNIFYING_GLASS), &tr!("Search commands   {}", keys), false) };
                if search.clicked() {
                    self.open_palette();
                }
                if !compact && self.settings.hud {
                    kit::vsep(ui, &t);
                    // the engine's rate, and frames that came late for the output's vsync (with the HUD)
                    let (fps, tip) = match &self.live {
                        Some(l) => {
                            let (late, us) = (l.engine.status.late.load(Relaxed), l.engine.status.frame_us.load(Relaxed));
                            let fps = if late > 0 { format!("{:.0} fps · {late} late", l.fps) } else { format!("{:.0} fps", l.fps) };
                            (fps, tr!("The engine: frames per second, frames late for the output; its last frame took {} ms on the render thread", format!("{:.2}", us as f32 / 1000.0)))
                        }
                        None => ("-- fps".to_string(), String::new()),
                    };
                    let r = ui.add(egui::Label::new(RichText::new(fps).font(Theme::mono(11.0)).color(t.fg_muted)).sense(egui::Sense::click()));
                    if r.clicked() {
                        self.settings.hud ^= true;
                    }
                    self.hint(&r, tip);
                }
            });
        });
        // a divider between the window's title and the bar (after the bar, which fills
        // its background over anything drawn before)
        ui.painter().hline(top.x_range(), top.top() + 0.5, Stroke::new(1.0, t.line));
    }

    /// The audio input: its name and a small live meter, or why nothing arrives; the menu
    /// switches inputs.
    fn audio_chip(&mut self, ui: &mut Ui, t: &Theme, tight: bool) {
        let Some(live) = &self.live else {
            ui.add_enabled_ui(false, |ui| kit::button(ui, t, Some(icons::SPEAKER_HIGH), tr!("Computer audio"), false));
            return;
        };
        let state = live.audio.state().unwrap_or_default();
        let (current, level) = (live.audio.input.clone(), live.engine.features.load()[0]);
        let (text, tip) = audio_words(&state, current.as_ref());
        // a narrow bar shortens the name
        let text = if tight && text.chars().count() > 16 { format!("{}…", text.chars().take(15).collect::<String>()) } else { text };
        // room on the right for the level: a ring whose middle fills with the sound
        let caret = format!("         {}", icons::CARET_DOWN);
        let r = kit::button(ui, t, Some(icons::SPEAKER_HIGH), &format!("{text}{caret}"), false);
        let caret_w = ui.painter().layout_no_wrap(icons::CARET_DOWN.to_string(), t.body(), t.fg_text).size().x;
        let center = egui::pos2(r.rect.right() - t.pad - caret_w - 15.0, r.rect.center().y);
        let live = state.hearing == Hearing::Live;
        ui.painter().circle_stroke(center, 6.0, Stroke::new(1.0, if live { t.accent.gamma_multiply(0.6) } else { t.fg_muted }));
        if live {
            let fill = level.clamp(0.0, 1.0).sqrt();
            ui.painter().circle_filled(center, 1.0 + 4.0 * fill, Theme::mix(t.fg_muted, t.accent, 0.4 + 0.6 * fill));
            // at the preview's rate, not the display's (a 144 Hz screen drew it 144 times a second)
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        self.hint(&r, tip);
        let mut picked = (None, false);
        kit::menu(t, &r).show(|ui| picked = audio_menu(ui, t, &state, current.as_ref()));
        self.pick_audio(ui.ctx(), picked);
    }

    /// An Audio In's input: the top bar's own (one input for now), with the same menu.
    pub fn audio_source(&mut self, ui: &mut Ui, t: &Theme) {
        let Some(live) = &self.live else { return };
        let (state, current) = (live.audio.state().unwrap_or_default(), live.audio.input.clone());
        let name = if state.name.is_empty() { tr!("Computer audio").to_string() } else { state.name.clone() };
        let r = kit::select(ui, t, Some(icons::SPEAKER_HIGH), &name).on_hover_text(tr!("The same input as the audio chip's, in the top bar"));
        let mut picked = (None, false);
        kit::menu(t, &r).show(|ui| picked = audio_menu(ui, t, &state, current.as_ref()));
        self.pick_audio(ui.ctx(), picked);
    }

    /// What an audio menu picked: an input to listen to, or the way to the permission.
    fn pick_audio(&mut self, ctx: &egui::Context, (pick, help): (Option<Input>, bool)) {
        if help {
            // Preferences, on its Audio page
            ctx.data_mut(|d| d.insert_temp(Id::unique("preferences page"), 2usize));
            self.show_settings = true;
        }
        if let (Some(input), Some(live)) = (pick, &mut self.live) {
            // the computer's or an app's sound: its tempo leads (nobody types a DJ's BPM)
            if matches!(input, Input::Computer | Input::App { .. }) {
                self.settings.auto_bpm = true;
            }
            self.settings.audio = input.clone();
            live.audio.listen(Some(input));
        }
    }

    /// The beat clock: its tempo and a dot on every beat. Click it (or T) on the beat to tap the
    /// tempo; its menu sets the tempo by hand and puts the downbeat on now.
    /// Auto-BPM (docs/audio.md §4): while it's on and no MIDI clock drives the beat clock, a
    /// confident estimate that agrees with the one before sets the tempo and nudges the beat
    /// onto the music's. Setting the tempo by hand turns it off: that's the lock.
    pub fn follow_music(&mut self) {
        if !self.settings.auto_bpm || self.midi.clock_seen.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
            return;
        }
        let Some(live) = &self.live else { return };
        // others on Link lead the tempo: the music's estimate steps back
        if live.link_peers().is_some_and(|n| n > 0) {
            return;
        }
        let Some(heard) = live.audio.state.lock().ok().and_then(|s| s.tempo) else { return };
        if self.auto.taken == Some(heard.beat) {
            return;
        }
        self.auto.taken = Some(heard.beat);
        let steady = self.auto.before.is_some_and(|b| (b - heard.bpm).abs() < 1.0);
        self.auto.before = Some(heard.bpm);
        if heard.confidence >= Auto::SURE && steady {
            live.follow(heard.bpm, None, heard.beat);
            self.auto.set = Some(std::time::Instant::now());
        }
    }

    fn tempo_chip(&mut self, ui: &mut Ui, t: &Theme) {
        let Some(live) = &self.live else {
            ui.add_enabled_ui(false, |ui| kit::button(ui, t, Some(icons::METRONOME), "-- BPM", false));
            return;
        };
        let [bpm, beat, _] = live.clock();
        // what leads the tempo: a MIDI clock while its beats come, else Link's peers, else the
        // music (the default), else a hand
        let following = self.midi.clock_seen.is_some_and(|at| at.elapsed() < Duration::from_secs(2));
        let peers = live.link_peers();
        let linked = !following && peers.is_some_and(|n| n > 0);
        let heard = live.audio.state.lock().ok().and_then(|s| s.tempo);
        let model = live.audio.settings.mode == velo_audio::Mode::Model;
        let auto = !following && !linked && self.settings.auto_bpm;
        let sure = auto && self.auto.set.is_some_and(|at| at.elapsed() < Duration::from_secs(3));
        let who = if following {
            "MIDI".to_string()
        } else if linked {
            format!("LINK {}", peers.unwrap_or(0))
        } else if sure {
            tr!("AUTO").to_string()
        } else if auto {
            tr!("LISTENING").to_string()
        } else {
            heard.map_or_else(|| tr!("BY HAND").to_string(), |h| tr!("{} HEARD", format!("{:.1}", h.bpm)))
        };
        let r = kit::button(ui, t, Some(icons::METRONOME), &format!("{bpm:.1} BPM  {}", icons::CARET_DOWN), following || sure || linked);
        let tip = if following {
            tr!("Following a MIDI clock: its tempo and its beats (Start puts the downbeat)").to_string()
        } else if let Some(n) = peers {
            if n > 0 { tr!("Ableton Link: tempo and bar shared with {} on the network", n) } else { tr!("Ableton Link is on: no one else on the network yet").to_string() }
        } else if auto {
            if sure { tr!("Following the music's tempo; set it by hand in the menu to lock it") } else { tr!("Listening for the music's tempo: not sure of it yet") }.to_string()
        } else {
            tr!("Set by hand. T taps it; the menu follows the music again").to_string()
        };
        let r = r.on_hover_text(tip);
        // a tap (here, T, a pad) flashes it
        if let Some(at) = live.tapped.get() {
            let k = 1.0 - at.elapsed().as_secs_f32() / 0.3;
            if k > 0.0 {
                ui.painter().rect_filled(r.rect.shrink(3.0), t.radius_sm - 2, t.accent.gamma_multiply(0.4 * k));
                ui.ctx().request_repaint();
            }
        }
        let learned = self.learnable(ui, &r, &velo_core::Target::TapTempo);
        let Some(live) = &self.live else { return };
        // the beat, and who leads
        let (dot, _) = ui.allocate_exact_size(vec2(8.0 + t.gap, t.control_h), Sense::hover());
        let dot = dot.with_min_x(dot.left() + t.gap);
        ui.painter().circle_filled(dot.center(), 3.0 + 3.0 * beat, t.accent.gamma_multiply(0.25 * beat));
        ui.painter().circle_filled(dot.center(), 3.0, Theme::mix(t.bg_control, t.accent, 0.3 + 0.7 * beat));
        // who leads it, as wide as it says (mono: the digits heard don't move what follows), and
        // the model's badge after it when the drum model hears the tempo and beats
        let said = ui.painter().layout_no_wrap(who.to_string(), Theme::mono(t.text_xs), t.fg_muted);
        let badge = (model && !following && !linked && (auto || heard.is_some())).then(|| widgets::model_badge_size(ui, t));
        let (slot, _) = ui.allocate_exact_size(vec2(t.gap + said.size().x + badge.map_or(0.0, |b| t.gap + b.x) + t.pad, t.control_h), Sense::hover());
        let text = egui::Rect::from_min_size(pos2(slot.left() + t.gap, slot.center().y - said.size().y / 2.0), said.size());
        ui.painter().galley(text.min, said, t.fg_muted);
        if let Some(b) = badge {
            ui.put(egui::Rect::from_center_size(pos2(text.right() + t.gap + b.x / 2.0, slot.center().y), b), |ui: &mut Ui| widgets::model_badge(ui, t));
        }
        let (mut set, mut resync, mut tap) = (None, false, false);
        let (mut auto_bpm, mut link) = (self.settings.auto_bpm, self.settings.link);
        if !learned {
            let popup = Popup::menu(&r).frame(kit::popover_frame(ui.ctx(), t).inner_margin(0)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(300.0);
            popup.show(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.gap, t.gap);
                let pad = egui::Margin::same(t.pad as i8);
                // the tempo now, and whether the drum model hears it
                egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr!("Tempo")).strong().color(t.fg_text));
                        if model {
                            widgets::model_badge(ui, t);
                        }
                        kit::right(ui, |ui| ui.label(RichText::new(format!("{bpm:.1} BPM")).font(Theme::mono(t.text_sm)).color(t.accent)));
                    });
                });
                kit::menu_line(ui, t);
                // who leads it
                egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let hearing = match heard {
                        Some(h) => tr!("Hearing {} BPM · {} % sure", format!("{:.1}", h.bpm), (h.confidence * 100.0).round()),
                        None => tr!("Listening… (8 s of music to tell)").to_string(),
                    };
                    kit::setting(ui, t, tr!("Follow the music"), &hearing, |ui| kit::switch(ui, t, &mut auto_bpm, tr!("Follow the music")));
                    // in builds with Ableton's SDK (the `link` feature)
                    if cfg!(feature = "link") {
                        let text = match peers {
                            Some(n) => tr!("{} on the network", n),
                            None => tr!("Tempo and bar with Live, DJ apps, phones").to_string(),
                        };
                        kit::setting(ui, t, tr!("Ableton Link"), &text, |ui| kit::switch(ui, t, &mut link, tr!("Ableton Link")));
                    }
                });
                kit::menu_line(ui, t);
                // by hand: a step either way, typed, tapped, the downbeat put
                egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    kit::heading(ui, t, icons::HAND_TAP, tr!("SET BY HAND"));
                    ui.horizontal(|ui| {
                        if kit::icon_button(ui, t, icons::MINUS, tr!("1 BPM slower"), false).clicked() {
                            set = Some(bpm - 1.0);
                        }
                        let mut typed = bpm;
                        let range = velo_engine::modulate::MIN_BPM..=velo_engine::modulate::MAX_BPM;
                        let w = ui.available_width() - t.control_h - t.gap;
                        if ui.add_sized(vec2(w, t.control_h), egui::DragValue::new(&mut typed).range(range).speed(0.1).fixed_decimals(1).suffix(" BPM")).changed() {
                            set = Some(typed);
                        }
                        if kit::icon_button(ui, t, icons::PLUS, tr!("1 BPM faster"), false).clicked() {
                            set = Some(bpm + 1.0);
                        }
                    });
                    let shown = kit::actions(ui, t, &[(kit::Variant::Outline, Some(icons::HAND_TAP), tr!("Tap")), (kit::Variant::Outline, Some(icons::ARROWS_CLOCKWISE), tr!("Resync"))]);
                    tap = shown[0].clone().on_hover_text(tr!("Tap on the beat (or T); the first tap is a downbeat")).clicked();
                    resync = shown[1].clone().on_hover_text(tr!("The downbeat is now")).clicked();
                });
                kit::menu_footer(ui, t, tr!("Setting it by hand stops following the music"));
            });
        }
        if tap {
            live.tap();
        }
        if let Some(bpm) = set {
            live.set_bpm(bpm);
        }
        if resync {
            live.resync();
        }
        if link != self.settings.link {
            live.set_link(link);
            self.settings.link = link;
        }
        // a hand on the tempo locks it: following the music steps back
        self.settings.auto_bpm = auto_bpm && set.is_none() && !resync && !tap;
    }

    fn midi_chip(&mut self, ui: &mut Ui, t: &Theme, icon_only: bool) {
        let learn_tip = self.tip(tr!("MIDI learn: click a control, then move a knob or hit a pad"), "stage.learn");
        let r = if icon_only { kit::icon_button(ui, t, icons::PIANO_KEYS, &learn_tip, self.learn.on) } else { kit::button(ui, t, Some(icons::PIANO_KEYS), "MIDI    ", self.learn.on) };
        // what comes in: a dot inside the button that flares with each message
        let k = ((self.midi.seen - ui.input(|i| i.time)) / 0.15).exp() as f32;
        let dot = if icon_only { pos2(r.rect.right() - 6.0, r.rect.top() + 6.0) } else { pos2(r.rect.right() - 12.0, r.rect.center().y) };
        ui.painter().circle_filled(dot, 3.0 + 3.0 * k, t.accent.gamma_multiply(0.25 * k));
        ui.painter().circle_filled(dot, if icon_only { 2.5 } else { 3.0 }, Theme::mix(t.bg_control, t.accent, 0.3 + 0.7 * k));
        if k > 0.01 {
            ui.ctx().request_repaint();
        }
        // the menu's caret joined to the button, a split button
        let menu = if icon_only {
            kit::icon_button(ui, t, icons::CARET_DOWN, tr!("MIDI: controllers, and what they're bound to"), false)
        } else {
            kit::group_line(ui, t);
            kit::button(ui, t, None, icons::CARET_DOWN, false).on_hover_text(tr!("MIDI: controllers, and what they're bound to"))
        };
        self.hint(&r, learn_tip);
        if r.clicked() {
            self.command("stage.learn");
        }
        let ports = self.midi.ports();
        let bindings = self.session.project.bindings.clone();
        let names: Vec<String> = bindings.iter().map(|b| self.target_name(&b.target)).collect();
        let (mut remove, mut any_port, mut behave) = (None, None, None);
        let (learning, mut learn) = (self.learn.on, false);
        // a header, the ports, the bindings, what the keys do: the dropdowns' layout
        let popup = Popup::menu(&menu).frame(kit::popover_frame(ui.ctx(), t).inner_margin(0)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(360.0);
        popup.show(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.gap, t.gap);
            let pad = egui::Margin::same(t.pad as i8);
            egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("MIDI").strong().color(t.fg_text));
                    kit::right(ui, |ui| learn = kit::button(ui, t, Some(icons::PIANO_KEYS), tr!("Learn"), learning).clicked());
                });
            });
            kit::menu_line(ui, t);
            egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                ui.set_width(ui.available_width());
                kit::heading(ui, t, icons::PLUGS, tr!("PORTS"));
                if ports.is_empty() {
                    kit::hint(ui, t, tr!("No controller found. Plug one in and it shows up here."));
                }
                for p in &ports {
                    ui.label(RichText::new(format!("{}  {p}", icons::PIANO_KEYS)).color(t.fg_text));
                }
            });
            kit::menu_line(ui, t);
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Frame::new().inner_margin(pad).show(ui, |ui| {
                    kit::heading(ui, t, icons::LINK, tr!("BINDINGS"));
                    if bindings.is_empty() {
                        kit::hint(ui, t, tr!("Nothing bound yet. Press L, click a control, then move a knob or hit a pad."));
                    }
                    for (i, (b, name)) in bindings.iter().zip(&names).enumerate() {
                        let Control::Midi { port, channel, msg } = &b.control;
                        // the control and its channel, what it turns on the right; under them what it's bound to
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(midi::control_label(&b.control)).font(Theme::mono(t.text_sm)).color(t.accent));
                            ui.label(RichText::new(tr!("ch {}", channel + 1)).small().color(t.fg_muted));
                            kit::right(ui, |ui| {
                                if kit::icon_button(ui, t, icons::X, tr!("Remove this binding"), false).clicked() {
                                    remove = Some(i);
                                }
                                match port {
                                    Some(p) => {
                                        if kit::button(ui, t, None, tr!("Any port"), false).on_hover_text(tr!("Bound to {}: make it answer the same control on any port", p)).clicked() {
                                            any_port = Some(i);
                                        }
                                    }
                                    None => {
                                        ui.label(RichText::new(tr!("any port")).small().color(t.fg_muted));
                                    }
                                }
                                // a CC on a knob or an endless encoder (learn guesses; this settles it)
                                if matches!(msg, velo_core::MidiMsg::Cc(_)) && matches!(b.target, velo_core::Target::Macro(_) | velo_core::Target::Param { .. }) {
                                    let now = match b.behavior {
                                        velo_core::Behavior::Relative { mode, .. } => midi::encoder_name(mode),
                                        _ => tr!("Knob").to_string(),
                                    };
                                    let r = kit::button(ui, t, None, &format!("{now}  {}", icons::CARET_DOWN), false);
                                    kit::menu(t, &r).show(|ui| {
                                        let mut items = vec![tr!("Knob").to_string()];
                                        items.extend(velo_core::RelMode::ALL.iter().map(|m| midi::encoder_name(*m)));
                                        let list: Vec<(&str, &str)> = items.iter().map(|s| ("", s.as_str())).collect();
                                        match kit::choose(ui, t, &list, None) {
                                            Some(0) => behave = Some((i, midi::behavior_for(&b.target, *msg))),
                                            Some(k) => behave = Some((i, velo_core::Behavior::Relative { mode: velo_core::RelMode::ALL[k - 1], step: 1.0 / 128.0 })),
                                            None => {}
                                        }
                                    });
                                }
                            });
                        });
                        ui.add(egui::Label::new(RichText::new(format!("→ {name}")).color(t.fg_text)).truncate());
                        if i + 1 < bindings.len() {
                            ui.add_space(t.gap);
                        }
                    }
                });
            });
            kit::menu_footer(ui, t, tr!("L: click a control, then move a knob or hit a pad"));
        });
        if learn {
            self.command("stage.learn");
        }
        if let Some(i) = remove {
            self.run("binding.remove", json!({ "index": i }));
        }
        if let Some((mut b, behavior)) = behave.and_then(|(i, behavior)| Some((bindings.get(i)?.clone(), behavior))) {
            b.behavior = behavior;
            self.run("binding.add", json!(b));
        }
        if let Some(mut b) = any_port.and_then(|i| bindings.get(i).cloned()) {
            let Control::Midi { port, .. } = &mut b.control;
            *port = None;
            self.run("binding.add", json!(b));
        }
    }

    /// Where the picture goes: a stage window on a display, or a window. Closing it is here too
    /// (not Esc: hard to do by accident).
    /// Where the picture goes; `icon_only` with no words, `blackout` with Blackout in its menu.
    fn output_chip(&mut self, ui: &mut Ui, t: &Theme, icon_only: bool, blackout: bool) {
        let Some(live) = &self.live else {
            ui.add_enabled_ui(false, |ui| kit::button(ui, t, Some(icons::PROJECTOR_SCREEN), if icon_only { "" } else { tr!("Output") }, false));
            return;
        };
        let dark = live.blackout;
        let shared = live.shared.clone();
        let (output, displays) = {
            let s = shared.borrow();
            (s.output.clone(), s.displays.clone())
        };
        let label = match &output {
            None => tr!("Output").to_string(),
            Some(Target::Window) => tr!("Window").to_string(),
            Some(Target::Display(name)) => display_name(name),
        };
        let text = if icon_only { icons::CARET_DOWN.to_string() } else { format!("{label}  {}", icons::CARET_DOWN) };
        // in Blackout, the menu holding it says so
        let r = kit::button(ui, t, Some(if blackout && dark { icons::POWER } else { icons::PROJECTOR_SCREEN }), &text, output.is_some() || live.sharing.is_some() || live.pattern || (blackout && dark));
        self.hint(&r, tr!("Where the picture goes: a display, fullscreen, or a window. Close it from here."));
        if r.clicked() {
            shared.borrow_mut().requests.push(Request::Displays);
        }
        let (mut request, mut stage, mut share) = (None, None, None);
        let (sharing, frozen, pattern) = (live.sharing, live.frozen, live.pattern);
        kit::menu(t, &r).show(|ui| {
            for d in &displays {
                let hz = d.hz.map_or(String::new(), |hz| format!(" · {hz:.0} Hz"));
                let target = Target::Display(d.name.clone());
                if ui.selectable_label(output.as_ref() == Some(&target), format!("{}  {}  {}×{}{hz}", icons::MONITOR, display_name(&d.name), d.size[0], d.size[1])).clicked() {
                    request = Some(Request::Open(target));
                }
            }
            if ui.selectable_label(output == Some(Target::Window), format!("{}  {}", icons::APP_WINDOW, tr!("Window"))).clicked() {
                request = Some(Request::Open(Target::Window));
            }
            ui.separator();
            if ui.add_enabled(output.is_some(), egui::Button::new(format!("{}  {}", icons::X, tr!("Close output")))).clicked() {
                request = Some(Request::Close);
            }
            // the program to other apps on this computer, or to the network, as well
            ui.separator();
            kit::menu_title(ui, t, tr!("SHARE"));
            let ways = [(Share::Spout, "Spout", tr!("To apps on this computer: Resolume, TouchDesigner, OBS, MadMapper")), (Share::Ndi, "NDI", tr!("To any computer on the network with an NDI receiver (OBS, vMix, NDI Studio Monitor)"))];
            for (kind, name, about) in ways.into_iter().filter(|w| w.0.available()) {
                let on = sharing == Some(kind);
                if ui.selectable_label(on, format!("{}  {}", icons::BROADCAST, name)).on_hover_text(about).clicked() {
                    share = Some((!on).then_some(kind));
                }
            }
            // the stage: a pattern to line up a projector, and Freeze and Blackout when the bar
            // has no room for them
            ui.separator();
            let keys = |id| self.settings.keymap.keys(id).first().map(keymap::text).unwrap_or_default();
            let mut items = vec![("stage.pattern", icons::CROSSHAIR, tr!("Test pattern"), pattern)];
            if blackout {
                items.extend([("stage.freeze", icons::SNOWFLAKE, tr!("Freeze"), frozen), ("stage.blackout", icons::POWER, tr!("Blackout"), dark)]);
            }
            for (id, icon, name, on) in items {
                let r = ui.add(egui::Button::selectable(on, format!("{icon}  {name}")).shortcut_text(keys(id)));
                let r = if id == "stage.pattern" { r.on_hover_text(tr!("A grid, a circle and color bars on the outputs, for lining up a projector")) } else { r };
                if r.clicked() {
                    stage = Some(id);
                }
            }
        });
        if let Some(request) = request {
            shared.borrow_mut().requests.push(request);
        }
        if let (Some(kind), Some(live)) = (share, &mut self.live) {
            live.share(kind);
        }
        if let Some(id) = stage {
            self.command(id);
        }
    }

    /// Where the picture shows now.
    fn preview_place(&self) -> Preview {
        match self.settings.split {
            crate::Split::Right => Preview::Beside,
            crate::Split::Bottom => Preview::Under,
            crate::Split::Off if self.settings.preview_background => Preview::Behind,
            crate::Split::Off if !self.settings.hide_preview => Preview::Window,
            crate::Split::Off => Preview::Off,
        }
    }

    /// The places the picture can show, one picked.
    fn preview_choices(&mut self, ui: &mut Ui) {
        let now = self.preview_place();
        let places = [
            (Preview::Window, icons::PICTURE_IN_PICTURE, tr!("Preview in a window")),
            (Preview::Behind, icons::STACK, tr!("Preview behind the canvas")),
            (Preview::Beside, icons::SQUARE_SPLIT_HORIZONTAL, tr!("Preview beside the nodes")),
            (Preview::Under, icons::LAYOUT, tr!("Preview under the nodes")),
            (Preview::Off, icons::X, tr!("No preview")),
        ];
        for (place, icon, label) in places {
            if ui.selectable_label(now == place, format!("{icon}  {label}")).clicked() {
                let s = &mut self.settings;
                s.split = match place {
                    Preview::Beside => crate::Split::Right,
                    Preview::Under => crate::Split::Bottom,
                    _ => crate::Split::Off,
                };
                s.preview_background = place == Preview::Behind;
                s.hide_preview = place != Preview::Window;
                ui.close();
            }
        }
    }

    /// A menu line for a keymap command, with its current shortcut.
    fn menu_command(&mut self, ui: &mut Ui, icon: &str, label: &str, command: &str) {
        let keys = self.settings.keymap.keys(command).first().map(keymap::text).unwrap_or_default();
        if ui.add(egui::Button::new(format!("{icon}  {label}")).shortcut_text(keys)).clicked() {
            self.command(command);
            ui.close();
        }
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        self.menu_command(ui, icons::FILE_PLUS, tr!("New"), "app.new");
        self.menu_command(ui, icons::FOLDER_OPEN, tr!("Open…"), "app.open");
        // the projects opened or saved lately, still there
        let recent: Vec<std::path::PathBuf> = self.settings.recent.iter().filter(|p| p.exists()).cloned().collect();
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button(format!("{}  {}", icons::FOLDER_OPEN, tr!("Open Recent")), |ui| {
                for path in recent {
                    let name = path.file_stem().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                    if ui.button(name).on_hover_text(path.display().to_string()).clicked() {
                        self.request(crate::files::Pending::Open(path));
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(tr!("Clear the list")).clicked() {
                    self.settings.recent.clear();
                    ui.close();
                }
            });
        });
        self.menu_command(ui, icons::FLOPPY_DISK, tr!("Save"), "app.save");
        self.menu_command(ui, icons::FLOPPY_DISK, tr!("Save As…"), "app.save_as");
        ui.separator();
        self.menu_command(ui, icons::FOLDER_OPEN, tr!("Import Scenes…"), "app.import");
        self.menu_command(ui, icons::FLOPPY_DISK, tr!("Export with Media…"), "app.export");
        self.menu_command(ui, icons::SPARKLE, tr!("Open the demo"), "app.demo");
        ui.separator();
        self.menu_command(ui, icons::KEYBOARD, tr!("Keyboard shortcuts"), "app.shortcuts");
        self.menu_command(ui, icons::GEAR_SIX, tr!("Preferences"), "app.preferences");
        ui.separator();
        // out of the project: back to the start screen's list, or out of Velo (unsaved work asks first)
        if ui.button(format!("{}  {}", icons::SQUARES_FOUR, tr!("Quit to project list"))).clicked() {
            self.request(crate::files::Pending::Start);
            ui.close();
        }
        if ui.button(format!("{}  {}", icons::POWER, tr!("Quit"))).clicked() {
            self.request(crate::files::Pending::Quit);
            ui.close();
        }
    }

    // ---- status bar and toasts ----------------------------------------------

    /// Velo's loads for the status bar, as far as this system tells them, each with what its
    /// hover says: the engine's rate, then the CPU, memory, GPU and video memory it takes.
    fn loads(&self) -> Vec<(widgets::Load, String)> {
        let u = &self.usage;
        let size = |mb: f32| if mb >= 1000.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.0} MB") };
        let mb = |bytes: u64| (bytes >> 20) as f32;
        let trail = |t: &std::collections::VecDeque<f32>| t.iter().copied().collect::<Vec<f32>>();
        // memory against its own highest with room above: a share of all of it would lie flat
        let relative = |t: &std::collections::VecDeque<f32>| {
            let top = t.iter().copied().fold(0.0, f32::max) * 1.25;
            t.iter().map(|v| if top > 0.0 { v / top } else { 0.0 }).collect::<Vec<f32>>()
        };
        let mut loads = Vec::new();
        if let (Some(live), Some(fps)) = (&self.live, u.fps) {
            let target = 60.0;
            let frame = live.engine.status.frame_us.load(Relaxed) as f32 / 1000.0;
            let tip = tr!("The engine: {} frames a second, {} ms a frame on its thread; the interface {} ms a frame", format!("{fps:.0}"), format!("{frame:.1}"), format!("{:.1}", self.cpu.stats().0 * 1000.0));
            let strain = 1.0 - fps / target;
            loads.push((widgets::Load { name: "FPS", value: fps, format: |v| format!("{v:.0}"), room: "120", strain, warn: 0.08, high: 0.25, trail: u.trails.fps.iter().map(|f| f / target).collect() }, tip));
        }
        if let Some(cpu) = u.cpu {
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
            loads.push((
                widgets::Load { name: "CPU", value: cpu, format: |v| format!("{v:.0}%"), room: "100%", strain: cpu / 100.0, warn: 0.6, high: 0.85, trail: trail(&u.trails.cpu) },
                tr!("Velo's share of the whole CPU ({} threads)", cores),
            ));
        }
        if let Some(memory) = u.memory {
            let strain = u.trails.memory.back().copied().unwrap_or(0.0);
            let tip = match u.memory_total {
                Some(total) => tr!("Velo's memory: {} of the {} this computer has", size(mb(memory)), size(mb(total))),
                None => tr!("Velo's memory: {}", size(mb(memory))),
            };
            loads.push((widgets::Load { name: "RAM", value: mb(memory), format: size, room: "999 MB", strain, warn: 0.5, high: 0.75, trail: relative(&u.trails.memory) }, tip));
        }
        if let Some(gpu) = u.gpu {
            let mut tip = tr!("How busy Velo keeps the GPU, as Task Manager counts it").to_string();
            if gpu >= 70.0 {
                tip = format!("{tip}. {}", tr!("Each node's card says what it costs a frame, in ms: the heavy ones show amber or red"));
            }
            loads.push((widgets::Load { name: "GPU", value: gpu, format: |v| format!("{v:.0}%"), room: "100%", strain: gpu / 100.0, warn: 0.7, high: 0.9, trail: trail(&u.trails.gpu) }, tip));
        }
        if let Some(vram) = u.vram {
            let strain = u.trails.vram.back().copied().unwrap_or(0.0);
            let mut tip = match u.vram_budget.filter(|b| *b > 0) {
                Some(budget) => tr!("Video memory Velo holds: {} of the {} the system lets it use", size(mb(vram)), size(mb(budget))),
                None => tr!("Video memory Velo holds: {}", size(mb(vram))),
            };
            // what the scenes kept ready take, and what they may
            if let Some(live) = &self.live {
                let (graphs, bytes) = live.held;
                tip = match u.vram_budget.filter(|b| *b > 0) {
                    Some(budget) => format!("{tip}. {}", tr!("{} scenes built, {} of the {} they may take", graphs, size(mb(bytes)), size(mb(budget / 2)))),
                    None => format!("{tip}. {}", tr!("{} scenes built, {}", graphs, size(mb(bytes)))),
                };
            }
            loads.push((widgets::Load { name: "VRAM", value: mb(vram), format: size, room: "999 MB", strain, warn: 0.7, high: 0.9, trail: relative(&u.trails.vram) }, tip));
        }
        loads
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        let t = self.theme.clone();
        kit::strip(ui, &t, 24.0, t.bg_panel, |ui| {
            // the app's load on the right, a chip each, close together
            let mut right = ui.max_rect().right();
            for (load, tip) in self.loads().iter().rev() {
                let r = widgets::load_chip(ui, &t, right, load);
                self.hint(&r, tip);
                right = r.rect.left() - 4.0;
            }
            ui.set_max_width(right - ui.max_rect().left() - t.pad);
            // what's under the mouse, else the two keys that find everything
            if self.info.is_empty() {
                let map = &self.settings.keymap;
                for (keys, what) in [(map.label("app.shortcuts"), tr!("Shortcuts")), (map.label("app.palette"), tr!("Commands"))] {
                    kbd(ui, &t, &keys);
                    ui.label(RichText::new(what).color(t.fg_muted).small());
                    ui.add_space(t.pad);
                }
            } else {
                ui.add(egui::Label::new(RichText::new(&self.info).color(t.fg_muted).small()).truncate());
            }
        });
    }

    /// While Velo starts or a project opens, Ableton's way: its mark, what it's doing and how
    /// far along, over everything. It fades once the engine renders, the scenes are built, the
    /// audio is open and every scene has been built once and pictured for its card: that last
    /// part is what would make a big project stutter right after opening, so it happens behind
    /// this. Without a GPU or a sound card it doesn't wait past six seconds; the scenes get two
    /// minutes; Esc or a click goes on at once.
    pub fn splash_ui(&mut self, ctx: &egui::Context) {
        let Some(fading) = self.splash else { return };
        let t = self.theme.clone();
        let now = ctx.input(|i| i.time);
        let waited = now - *self.splash_from.get_or_insert(now);
        let hold = std::env::var_os("VELO_DEMO_SPLASH").is_some();
        let live = self.live.as_ref();
        let scenes = self.session.project.scenes.len();
        let pictured = live.map_or(scenes, |l| l.pictured(&self.session.project)).min(scenes);
        let steps = [
            (tr!("Starting the engine").to_string(), live.is_some_and(|l| l.engine.status.frames.load(Relaxed) > 0)),
            (tr!("Building the scenes").to_string(), self.session.active().is_none() || live.is_some_and(crate::live::Live::settled)),
            (tr!("Opening the audio").to_string(), live.is_some_and(|l| l.audio.state().is_some())),
            (tr!("Preparing the scenes · {} of {}", pictured, scenes), pictured >= scenes),
        ];
        let done = steps.iter().filter(|s| s.1).count();
        let started = steps[..3].iter().all(|s| s.1);
        let skip = started && ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.any_click());
        let ready = (done == steps.len() || live.is_none() || (!started && waited > 6.0) || waited > 120.0 || skip) && !hold;
        // shown long enough to read, then a short fade
        let fade_from = match fading {
            Some(at) => at,
            None if ready => {
                let at = now.max(now - waited + 0.9);
                self.splash = Some(Some(at));
                at
            }
            None => f64::INFINITY,
        };
        let alpha = (1.0 - ((now - fade_from) / 0.35) as f32).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            (self.splash, self.splash_from) = (None, None);
            return;
        }
        ctx.request_repaint();
        let screen = ctx.content_rect();
        egui::Area::new(Id::unique("splash")).order(egui::Order::Tooltip).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(alpha);
            painter.rect_filled(rect, 0.0, t.bg_base);
            let c = rect.center() - vec2(0.0, 40.0);
            let mark = egui::Rect::from_center_size(c, vec2(96.0, 96.0));
            painter.image(widgets::brand(ctx, widgets::ICON_256), mark, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Theme::NO_TINT);
            painter.text(c + vec2(0.0, 72.0), Align2::CENTER_CENTER, "Velo", FontId::proportional(28.0), t.fg_text);
            painter.text(c + vec2(0.0, 98.0), Align2::CENTER_CENTER, env!("CARGO_PKG_VERSION"), Theme::mono(t.text_xs), t.fg_muted);
            // the bar fills step by step (the last one scene by scene), eased toward where it's at
            let at = (done.min(3) as f32 + if started { pictured as f32 / scenes.max(1) as f32 } else { 0.0 }) / steps.len() as f32;
            let shown = ctx.animate_value_with_time(Id::unique("splash progress"), at, 0.3);
            let bar = egui::Rect::from_center_size(c + vec2(0.0, 132.0), vec2(220.0, 3.0));
            painter.rect_filled(bar, 2.0, t.bg_control);
            painter.rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * shown, bar.height())), 2.0, t.accent);
            let doing = steps.iter().find(|s| !s.1).map_or_else(|| tr!("Ready").to_string(), |s| format!("{}…", s.0));
            painter.text(c + vec2(0.0, 150.0), Align2::CENTER_CENTER, doing, t.small(), t.fg_muted);
            if started && done < steps.len() {
                painter.text(c + vec2(0.0, 172.0), Align2::CENTER_CENTER, tr!("Esc or a click goes on without waiting"), t.small(), t.fg_muted.gamma_multiply(0.7));
            }
        });
    }

    /// The start screen, before a project is open: the recent projects to pick up again, a new
    /// one, one from a file, the demo. Esc (or its close button) leaves an empty project.
    pub fn start_ui(&mut self, ctx: &egui::Context) {
        if !self.start {
            return;
        }
        let t = self.theme.clone();
        let screen = ctx.content_rect();
        let recent: Vec<std::path::PathBuf> = self.settings.recent.iter().filter(|p| p.exists()).cloned().collect();
        let mut pick: Option<crate::files::Pending> = None;
        let mut forget: Option<std::path::PathBuf> = None;
        let (has_recovery, mut restore, mut discard) = (self.recovery.is_some(), false, false);
        let mut open = false;
        let mut leave = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        egui::Area::new(Id::unique("start")).order(egui::Order::Middle).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, t.bg_base);
            let width = (rect.width() - 64.0).min(680.0);
            let column = egui::Rect::from_min_size(pos2(rect.center().x - width / 2.0, rect.top() + (rect.height() * 0.12).max(24.0)), vec2(width, rect.height() * 0.88 - 24.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(column).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // the mark, the name and the motto; the way out on the right
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.pad * 1.25;
                    let (mark, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                    ui.painter().image(widgets::brand(ctx, widgets::ICON_256), mark, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Theme::NO_TINT);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.add_space(3.0);
                        ui.label(RichText::new("Velo").size(24.0).strong().color(t.fg_text));
                        ui.label(RichText::new(crate::canvas::MOTTO).color(t.fg_muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        leave |= kit::icon_button(ui, &t, icons::X, tr!("Start with an empty project (Esc)"), false).clicked();
                        ui.label(RichText::new(env!("CARGO_PKG_VERSION")).font(Theme::mono(t.text_xs)).color(t.fg_muted));
                    });
                });
                ui.add_space(t.pad * 2.5);
                // what to start from, side by side
                let starts = [
                    (icons::FILE_PLUS, tr!("New project"), tr!("Start from an empty canvas")),
                    (icons::FOLDER_OPEN, tr!("Open…"), tr!("A project on this computer")),
                    (icons::PLAY, tr!("Open the demo"), tr!("See what Velo does, live")),
                ];
                let gap = t.pad;
                let tile = vec2((width - 2.0 * gap) / 3.0, 84.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for (i, (icon, title, what)) in starts.into_iter().enumerate() {
                        let (r, resp) = ui.allocate_exact_size(tile, Sense::click());
                        let h = kit::hover(ui, &t, &resp);
                        let painter = ui.painter();
                        painter.rect(r, CornerRadius::same(t.radius_md), Theme::mix(t.bg_panel, t.fg_text, 0.03 + 0.05 * h), Stroke::new(1.0, Theme::mix(t.line, t.accent, h * 0.7)), egui::StrokeKind::Inside);
                        kit::icon_at(painter, r.left_top() + vec2(22.0, 24.0), icon, 18.0, Theme::mix(t.fg_muted, t.accent, 0.4 + 0.6 * h));
                        painter.text(r.left_top() + vec2(14.0, 50.0), Align2::LEFT_CENTER, title, t.body(), t.fg_text);
                        painter.text(r.left_top() + vec2(14.0, 68.0), Align2::LEFT_CENTER, what, t.small(), t.fg_muted);
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            match i {
                                0 => pick = Some(crate::files::Pending::New),
                                1 => open = true,
                                _ => pick = Some(crate::files::Pending::Demo),
                            }
                        }
                    }
                });
                ui.add_space(t.pad * 2.5);
                // work Velo closed with unsaved last time: one more thing to open, or to throw
                // away (held: it can't be had back)
                if has_recovery {
                    egui::Frame::new().fill(t.accent.gamma_multiply(0.10)).stroke(Stroke::new(1.0, t.accent.gamma_multiply(0.35))).corner_radius(t.radius_md).inner_margin(t.pad).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = t.pad;
                            let (spot, _) = ui.allocate_exact_size(vec2(28.0, 36.0), Sense::hover());
                            kit::icon_at(ui.painter(), spot.center(), icons::CLOCK_COUNTER_CLOCKWISE, 18.0, t.accent);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new(tr!("Unsaved work from last time")).font(t.body()).color(t.fg_text));
                                ui.label(RichText::new(tr!("Velo closed before it was saved: pick it up where you left off.")).font(t.small()).color(t.fg_muted));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.allocate_ui(vec2(190.0, t.control_h + 4.0), |ui| {
                                    discard = kit::hold_button(ui, &t, Some(icons::TRASH), tr!("Hold to discard"), tr!("Discarded"), 1.0);
                                });
                                restore = kit::action(ui, &t, kit::Variant::Primary, Some(icons::CLOCK_COUNTER_CLOCKWISE), tr!("Restore")).clicked();
                            });
                        });
                    });
                    ui.add_space(t.pad * 2.0);
                }
                kit::caps(ui, &t, tr!("RECENT PROJECTS"));
                ui.add_space(t.gap + 2.0);
                if recent.is_empty() {
                    kit::hint(ui, &t, tr!("Projects you open or save show up here."));
                }
                kit::scroll(ui, &t, "start recent", false, t.bg_base, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for path in &recent {
                        let name = path.file_stem().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
                        let h = kit::hover(ui, &t, &resp);
                        let painter = ui.painter();
                        painter.rect_filled(r, CornerRadius::same(t.radius_md), t.fg_text.gamma_multiply(0.06 * h));
                        // a wash of its own, from its name: the same every time
                        let hue = name.bytes().fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(u32::from(b))) % 360;
                        let (a, _) = widgets::wash(hue as f32 / 360.0);
                        let swatch = egui::Rect::from_center_size(r.left_center() + vec2(26.0, 0.0), vec2(36.0, 36.0));
                        painter.rect_filled(swatch, CornerRadius::same(t.radius_sm), a.gamma_multiply(0.55));
                        kit::icon_at(painter, swatch.center(), icons::STACK, 16.0, t.fg_text.gamma_multiply(0.85));
                        painter.text(r.left_center() + vec2(56.0, -8.0), Align2::LEFT_CENTER, &name, t.body(), t.fg_text);
                        let folder = path.parent().map(tilde).unwrap_or_default();
                        let when = std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok()).map(|age| ago(age.as_secs())).unwrap_or_default();
                        let when = painter.text(r.right_center() - vec2(14.0, 0.0), Align2::RIGHT_CENTER, when, t.small(), t.fg_muted);
                        // the folder, cut short before the date
                        let room = when.left() - (r.left() + 56.0) - t.pad;
                        let galley = painter.layout(folder, t.small(), t.fg_muted, f32::INFINITY);
                        painter.with_clip_rect(egui::Rect::from_min_size(r.left_center() + vec2(56.0, 0.0), vec2(room.max(0.0), 20.0))).galley(r.left_center() + vec2(56.0, 2.0), galley, t.fg_muted);
                        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(path.display().to_string());
                        if resp.clicked() {
                            pick = Some(crate::files::Pending::Open(path.clone()));
                        }
                        // right-click: off the list (its file stays where it is), held to mean it
                        kit::context_menu(&t, &resp).show(|ui| {
                            if kit::hold_button(ui, &t, Some(icons::TRASH), tr!("Hold to remove from the list"), tr!("Removed"), 0.8) {
                                forget = Some(path.clone());
                                ui.close();
                            }
                        });
                    }
                });
            });
        });
        if open {
            self.open_dialog();
        }
        if let Some(path) = forget {
            self.settings.recent.retain(|p| *p != path);
        }
        if let Some(action) = pick {
            self.request(action);
        }
        if restore {
            self.restore_recovery();
        }
        if discard {
            self.recovery = None;
        }
        // a project loaded closes it (files.rs); so does leaving
        if leave {
            self.start = false;
        }
        // left for another project with that work still unopened: let go (its file stays until
        // this session's first autosave takes its place), so this session autosaves as usual
        if !self.start {
            self.recovery = None;
        }
    }

    /// The toast, Sileo's (hiaaryan/sileo, MIT) redrawn in egui: a pill with a tinted badge and
    /// title in the bottom-right corner; the message drops out of it into a blob that folds back
    /// before the toast goes. Hovering holds it open, a click sends it away.
    pub fn toast_ui(&mut self, ctx: &egui::Context) {
        let t = &self.theme;
        let Some(toast) = &mut self.toast else { return };
        // its life, the message folding back as long before the end as usual
        let (life, collapse_at) = (toast.life, toast.life - (LIFE - COLLAPSE_AT));
        let now = Instant::now();
        let age = (now - toast.born).as_secs_f32();
        if age > life + ENTER {
            self.toast = None;
            return;
        }
        let (color, icon, title) = match toast.tone {
            Tone::Done => (Theme::TONE_DONE, icons::CHECK, tr!("Done")),
            Tone::Action => (Theme::TONE_ACTION, icons::LIGHTNING, tr!("Your turn")),
            Tone::Error => (Theme::TONE_ERROR, icons::X, tr!("Error")),
        };
        // at the top of the window, in its middle, under the top bar: over whatever view is up
        // (the start screen, a panel, the canvas), where the eye already is. `a` is its body's
        // top right corner.
        let window = ctx.content_rect();
        let a = pos2(window.center().x + WIDTH / 2.0, window.top() + t.bar_h + t.pad + PILL_H);
        egui::Area::new(Id::unique("toast")).order(egui::Order::Foreground).fixed_pos(a - vec2(WIDTH, PILL_H)).show(ctx, |ui| {
            let painter = ui.painter();
            let font = FontId::proportional(t.text_md);
            let name = painter.layout_no_wrap(title.to_string(), font.clone(), color);
            let mut job = egui::text::LayoutJob::single_section(toast.text.clone(), egui::TextFormat { font_id: font, color: Theme::TOAST_TEXT, line_height: Some(LINE_H), ..Default::default() });
            job.wrap.max_width = WIDTH - 2.0 * PAD;
            let message = painter.layout_job(job);
            // the pill: the badge, the title, room at its round end
            let pill_w = 6.0 + 18.0 + 7.0 + name.size().x + 14.0;
            let action = toast.action.as_ref().map(|(label, _)| painter.layout_no_wrap(label.clone(), FontId::proportional(t.text_md), t.accent));
            let button_h = action.as_ref().map_or(0.0, |g| g.size().y + 8.0 + 12.0);
            let body_h = (PAD_TOP + message.size().y + PAD + button_h).max(PILL_H * 1.5);

            let open = toast.openness(now);
            let b = body_h * open.max(0.0);
            // how far the pill has melted into the body
            let m = (b / ROUND).clamp(0.0, 1.0);
            // the body drops down from under the header, which stays where it is, on the toast's side
            let body = egui::Rect::from_min_max(pos2(a.x - WIDTH, a.y), pos2(a.x, a.y + b));
            let px = body.left() + (WIDTH - pill_w) * SIDE;
            let pill = egui::Rect::from_min_max(pos2(px, body.top() - PILL_H), pos2(px + pill_w, body.top() + 2.0 * m));

            // its button, under the message on the right
            let button = action.as_ref().filter(|_| b > body_h * 0.8).map(|g| egui::Rect::from_min_size(pos2(body.right() - PAD - g.size().x - 12.0, body.bottom() - PAD - g.size().y - 8.0), g.size() + vec2(12.0, 8.0)));
            let pressed = button.is_some_and(|r| ui.interact(r, Id::unique("toast-action"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked());
            if pressed {
                toast.acted = toast.action.as_ref().map(|a| a.1);
                toast.born = now - Duration::from_secs_f32(life.max(age));
            }
            let hit = ui.interact(pill, Id::unique("toast-pill"), Sense::click());
            let hit_body = ui.interact(if b > 1.0 { body } else { egui::Rect::NOTHING }, Id::unique("toast-body"), Sense::click());
            let hovered = hit.hovered() || hit_body.hovered();
            if hit.clicked() || hit_body.clicked() {
                toast.born = now - Duration::from_secs_f32(life.max(age));
            } else if hovered && age > collapse_at - 1.0 && age < life {
                // held open: the clock waits a second short of folding back
                toast.born = now - Duration::from_secs_f32(collapse_at - 1.0);
            }
            let want = hovered && age < life || (EXPAND_AT..collapse_at).contains(&age);
            if want != toast.open {
                (toast.from, toast.open, toast.turned) = (open, want, now);
            }

            // in: down from 6 px above and from 95 %; out: the same, backwards
            let (k_in, k_out) = (kit::spring(age / ENTER), kit::ease((age - life) / ENTER));
            let opacity = kit::ease(age / ENTER) * (1.0 - k_out);
            let scale = 0.95 + 0.05 * k_in - 0.05 * k_out;
            let center = pill.center();
            let shift = vec2(0.0, -6.0 * (1.0 - k_in) - 6.0 * k_out);
            let at = egui::emath::TSTransform::new(center.to_vec2() * (1.0 - scale) + shift, scale);
            ui.multiply_opacity(opacity);
            ui.with_visual_transform(at, |ui| {
                let painter = ui.painter();
                let fill = t.toast;
                let r = |x: f32| x.round() as u8;
                let low = r(ROUND * (1.0 - m));
                painter.rect_filled(pill, CornerRadius { nw: r(ROUND), ne: r(ROUND), sw: low, se: low }, fill);
                if b > 0.5 {
                    // square where the pill sits flush on the body's edge, round elsewhere
                    let c = r(ROUND.min(b / 2.0));
                    let (flush_l, flush_r) = (pill.left() - body.left() < 0.5, body.right() - pill.right() < 0.5);
                    let (nw, ne) = (if flush_l { 0 } else { c }, if flush_r { 0 } else { c });
                    painter.rect_filled(body, CornerRadius { nw, ne, sw: c, se: c }, fill);
                    // Sileo's gooey filter rounds the inside corners where the pill meets the body
                    let f = FILLET.min(b / 2.0) * m;
                    for (x, out, flush) in [(pill.left(), -1.0, flush_l), (pill.right(), 1.0, flush_r)] {
                        if flush || f < 0.5 {
                            continue;
                        }
                        let corner = pos2(x, body.top());
                        let o = corner + vec2(out * f, -f);
                        // a fan from the corner (1 px into both shapes so no seam shows); epaint
                        // fans from the first point of a clockwise path, so the left one runs backwards
                        let mut edge = vec![corner + vec2(-out, -f)];
                        edge.extend((0..=8).map(|i| {
                            let a = i as f32 / 8.0 * FRAC_PI_2;
                            o + f * vec2(-out * a.cos(), a.sin())
                        }));
                        edge.push(corner + vec2(out * f, 1.0));
                        if out < 0.0 {
                            edge.reverse();
                        }
                        let fillet = std::iter::once(corner + vec2(-out, 1.0)).chain(edge).collect();
                        painter.add(egui::Shape::convex_polygon(fillet, fill, Stroke::NONE));
                    }
                    // the message shows once the body is mostly out
                    let k = ((open - 0.6) / 0.4).clamp(0.0, 1.0);
                    if k > 0.0 {
                        let mut clipped = painter.with_clip_rect(body);
                        clipped.multiply_opacity(k);
                        clipped.galley(body.min + vec2(PAD, PAD_TOP), message, Theme::TOAST_TEXT);
                        if let (Some(r), Some(g)) = (button, action) {
                            clipped.rect_filled(r, CornerRadius::same(t.radius_sm), t.accent.gamma_multiply(0.15));
                            clipped.galley(r.min + vec2(6.0, 4.0), g, t.accent);
                        }
                    }
                }
                let badge = pos2(pill.left() + 6.0 + 9.0, pill.top() + PILL_H / 2.0);
                painter.circle_filled(badge, 9.0, color.gamma_multiply(0.2));
                kit::icon_at(painter, badge, icon, 12.0, color);
                painter.galley(pos2(badge.x + 9.0 + 7.0, badge.y - name.size().y / 2.0), name, color);
            });
        });
        // frames while something moves; in between, wake for the next step
        let toast = self.toast.as_ref().expect("still there");
        let settled = age > EXPAND_AT + SPRING && (now - toast.turned).as_secs_f32() > SPRING && age < life;
        let next = [collapse_at, life].into_iter().find(|&at| at > age).unwrap_or(age);
        ctx.request_repaint_after(Duration::from_secs_f32(if settled { next - age } else { 0.0 }));
    }

    // ---- preferences (floating, not modal) ----------------------------------

    /// Preferences: a modal with its sections down a sidebar (General, Appearance, Audio, AI
    /// and agents, Control, Code, Updates, About), the picked one on the right.
    pub fn preferences(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let t = self.theme.clone();
        let pages = [
            (icons::GEAR_SIX, tr!("General")),
            (icons::SPEAKER_HIGH, tr!("Audio")),
            (icons::ROBOT, tr!("AI and agents")),
            (icons::BROADCAST, tr!("Control")),
            (icons::INFO, tr!("About")),
        ];
        let key = Id::unique("preferences page");
        // (VELO_DEMO_PREFS=n: opened on page n, for screenshots)
        let first = std::env::var("VELO_DEMO_PREFS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let mut page = ctx.data(|d| d.get_temp::<usize>(key)).unwrap_or(first).min(pages.len() - 1);
        let width = (ctx.content_rect().width() - 64.0).min(860.0);
        let height = (ctx.content_rect().height() - 160.0).clamp(300.0, 600.0);
        // the page turned to fades in and settles up a few pixels
        let (turned_key, now) = (Id::unique("preferences turned"), ctx.input(|i| i.time));
        let turned = ctx.data(|d| d.get_temp::<f64>(turned_key)).unwrap_or(now - 1.0);
        let k = if t.motion > 0.0 { kit::ease(((now - turned) / 0.22) as f32) } else { 1.0 };
        let was = page;
        let ((), close) = kit::modal_flush(ctx, &t, "preferences", tr!("Preferences"), width, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                // the sections, down the left
                ui.allocate_ui_with_layout(vec2(190.0, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_size(vec2(190.0, height));
                    // darker than the page beside it: the two are told apart by that, not by a line
                    let side = egui::Rect::from_min_size(ui.max_rect().min, vec2(190.0, height));
                    ui.painter().rect_filled(side, CornerRadius { sw: t.radius_md, ..CornerRadius::ZERO }, t.bg_base);
                    ui.add_space(t.gap * 2.0);
                    for (i, (icon, label)) in pages.iter().enumerate() {
                        let r = kit::row(ui, &t, Id::unique(("preferences row", i)), i == page);
                        let painter = ui.painter();
                        kit::icon_at(painter, pos2(r.rect.left() + 20.0, r.rect.center().y), icon, 15.0, if i == page { t.accent } else { t.fg_muted });
                        painter.text(pos2(r.rect.left() + 38.0, r.rect.center().y), Align2::LEFT_CENTER, *label, t.body(), if i == page { t.fg_text } else { t.fg_muted });
                        if r.clicked() {
                            page = i;
                        }
                    }
                    // which Velo this is, at the foot
                    let foot = pos2(ui.max_rect().left() + 20.0, ui.max_rect().top() + height - 18.0);
                    ui.painter().text(foot, Align2::LEFT_CENTER, concat!("Velo ", env!("CARGO_PKG_VERSION")), Theme::mono(t.text_xs), t.fg_muted.gamma_multiply(0.7));
                });
                // the picked one
                ui.allocate_ui_with_layout(vec2(ui.available_width(), height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_size(vec2(ui.available_width(), height));
                    // the pages' text wraps (the modal truncates by default)
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    kit::scroll(ui, &t, ("prefs", page), true, t.bg_card, |ui| {
                        ui.multiply_opacity(k);
                        ui.add_space(t.gap + 8.0 * (1.0 - k));
                        match page {
                            // General: the language, the theme, motion, the shader editor
                            0 => {
                                self.prefs_general(ui, &t);
                                self.prefs_theme(ui, &t);
                                self.prefs_code(ui, &t);
                            }
                            1 => self.prefs_audio(ui, &t),
                            2 => self.prefs_agents(ui, &t),
                            3 => self.prefs_osc(ui, &t),
                            // About: which Velo this is, its updates, who makes it
                            _ => self.prefs_about(ui, &t),
                        }
                    });
                });
            });
        });
        ctx.data_mut(|d| d.insert_temp(key, page));
        if page != was {
            ctx.data_mut(|d| d.insert_temp(turned_key, now));
        }
        if k < 1.0 || page != was {
            ctx.request_repaint();
        }
        if close {
            self.show_settings = false;
        }
    }

    /// Where macOS lets apps hear the computer: System Settings' Screen & System Audio Recording.
    pub fn open_audio_privacy() {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture").spawn();
    }

    /// Audio: following the music, and on macOS the permission to hear the computer, step by step.
    fn prefs_audio(&mut self, ui: &mut Ui, t: &Theme) {
        kit::page_title(ui, t, icons::SPEAKER_HIGH, tr!("Audio"), tr!("How Velo follows the music, and what it's allowed to hear."));
        kit::card(ui, t, tr!("TEMPO"), |ui| {
            let detail = tr!("On by default when Velo hears the computer or an app. Setting the tempo by hand turns it off.");
            kit::setting(ui, t, tr!("Follow the music's tempo"), detail, |ui| kit::switch(ui, t, &mut self.settings.auto_bpm, tr!("Follow the music's tempo")));
        });
        // only macOS asks for a permission to hear the computer
        if !cfg!(target_os = "macos") {
            return;
        }
        kit::card(ui, t, tr!("HEARING THE COMPUTER (MACOS)"), |ui| {
            let text = tr!(
                "Velo hears what the computer plays only with your permission.\n1. Open System Settings (the button below takes you there).\n2. Privacy & Security, then Screen & System Audio Recording.\n3. Under System Audio Recording Only, switch Velo on. If it isn't listed, click +, pick Velo in your Applications folder and switch it on.\n4. Quit Velo and open it again.\nFor a microphone or an audio interface: Privacy & Security, then Microphone, Velo on.\nEvery new build of Velo may ask again: macOS ties the permission to the build. If it stops hearing after an update, switch Velo off and on there, or remove it with - and add it again."
            );
            // its first line says why, the numbered ones are the steps, the rest is what else to know
            let mut lines = text.lines();
            if let Some(why) = lines.next() {
                kit::para(ui, t, why, t.fg_text);
                ui.add_space(t.gap);
            }
            let (numbered, notes): (Vec<&str>, Vec<&str>) = lines.partition(|l| l.chars().next().is_some_and(|c| c.is_ascii_digit()));
            let numbered: Vec<&str> = numbered.iter().map(|l| l.split_once(". ").map_or(*l, |(_, words)| words)).collect();
            kit::steps(ui, t, &numbered);
            ui.add_space(t.gap);
            if kit::button(ui, t, Some(icons::GEAR_SIX), tr!("Open System Settings"), false).clicked() {
                Self::open_audio_privacy();
            }
            ui.add_space(t.pad);
            for (i, line) in notes.iter().enumerate() {
                let (icon, color) = if i == 0 { (icons::MICROPHONE, t.accent) } else { (icons::INFO, t.warn) };
                kit::callout(ui, t, icon, line, color);
                ui.add_space(t.gap);
            }
            ui.add_space(t.pad - t.gap);
        });
    }

    fn prefs_general(&mut self, ui: &mut Ui, t: &Theme) {
        // each language under its own name, so it's found whatever the current one is
        kit::page_title(ui, t, icons::GEAR_SIX, tr!("General"), tr!("Velo's language, its looks, and the editor your shaders open in."));
        kit::card(ui, t, tr!("LANGUAGE"), |ui| {
            let current = Lang::ALL.iter().position(|l| *l == self.settings.language).unwrap_or(0);
            // a list to pick from: it stays one row however many languages there are
            let r = kit::select(ui, t, Some(icons::TRANSLATE), Lang::ALL[current].name());
            let mut picked = None;
            kit::menu(t, &r).show(|ui| {
                let items: Vec<(&str, &str)> = Lang::ALL.iter().map(|l| ("", l.name())).collect();
                picked = kit::choose(ui, t, &items, Some(current));
            });
            if let Some(i) = picked {
                self.settings.language = Lang::ALL[i];
                i18n::set(Lang::ALL[i]);
            }
            ui.add_space(t.pad);
        });
        kit::card(ui, t, tr!("MOTION"), |ui| {
            let mut reduce = self.settings.reduce_motion;
            let detail = tr!("Panels, menus and lists appear without animating.");
            if kit::setting(ui, t, tr!("Reduce motion"), detail, |ui| kit::switch(ui, t, &mut reduce, tr!("Reduce motion"))).changed() {
                self.settings.reduce_motion = reduce;
                self.use_theme(ui.ctx(), self.settings.theme.clone());
            }
        });
    }

    /// Preferences → About: who makes Velo, its source, and its sister project, as Ravune's About
    /// names Velo.
    fn prefs_about(&mut self, ui: &mut Ui, t: &Theme) {
        // the mark, the name, the motto, which Velo this is
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.pad * 1.5;
            let (mark, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
            ui.painter().image(widgets::brand(ui.ctx(), widgets::ICON_256), mark, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Theme::NO_TINT);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add_space(4.0);
                ui.label(RichText::new("Velo").size(24.0).strong().color(t.fg_text));
                ui.label(RichText::new(crate::canvas::MOTTO).color(t.fg_muted));
                ui.label(RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).font(Theme::mono(t.text_xs)).color(t.fg_muted));
            });
        });
        ui.add_space(t.pad * 2.0);
        self.prefs_updates(ui, t);
        kit::card(ui, t, tr!("ABOUT"), |ui| {
            kit::para(ui, t, tr!("Live visuals played by the music."), t.fg_text);
            ui.label(RichText::new(tr!("Made by gilgil")).strong().color(t.accent));
            ui.add_space(t.gap);
            ui.hyperlink_to(format!("{}  {}", icons::ARROW_SQUARE_OUT, tr!("Source on GitHub")), "https://github.com/g1lg1l/velo");
            ui.add_space(t.gap);
            kit::hint(ui, t, tr!("Free and open source: the app under GPL-3.0-or-later, its libraries MIT or Apache-2.0."));
            ui.add_space(t.pad);
        });
        kit::card(ui, t, tr!("SISTER PROJECT"), |ui| {
            kit::para(ui, t, tr!("Making the beats too? Ravune, from the same maker, is a modular DAW for deep sound design."), t.fg_text);
            ui.hyperlink_to(format!("{}  {}", icons::ARROW_SQUARE_OUT, tr!("Ravune · a modular DAW")), "https://github.com/g1lg1l/ravune");
            ui.add_space(t.pad);
        });
    }

    fn prefs_theme(&mut self, ui: &mut Ui, t: &Theme) {
        kit::card(ui, t, tr!("THEME"), |ui| {
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let current = self.settings.theme.name.clone();
                let mut picked = None;
                for theme in Theme::builtins().into_iter().chain(self.settings.themes.iter().cloned()) {
                    if theme_row(ui, t, &theme, theme.name == current).clicked() {
                        picked = Some(theme);
                    }
                }
                if let Some(theme) = picked {
                    self.use_theme(ui.ctx(), theme);
                }
            });
            ui.add_space(t.gap);
            if kit::button(ui, t, Some(icons::FOLDER_OPEN), tr!("Import…"), false).clicked() {
                self.import_theme(ui.ctx());
            }
            ui.add_space(t.pad);
        });
    }

    /// Switches to `theme`.
    pub fn use_theme(&mut self, ctx: &egui::Context, theme: Theme) {
        self.settings.theme = theme;
        self.theme = self.settings.effective_theme();
        self.theme.apply(ctx);
    }

    fn import_theme(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter(tr!("Velo theme"), &[THEME_EXTENSION]).pick_file() else { return };
        let read = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|text| ron::from_str::<Theme>(&text).map_err(|e| e.to_string()));
        match read {
            Ok(theme) => {
                self.settings.themes.retain(|t| t.name != theme.name);
                self.settings.themes.push(theme.clone());
                self.say(Tone::Done, tr!("Imported the theme \"{}\"", theme.name));
                self.use_theme(ctx, theme);
            }
            Err(e) => self.say(Tone::Error, tr!("Can't read {}: {}", path.display(), e)),
        }
    }

    // ---- shortcut sheet -----------------------------------------------------

    pub fn shortcut_sheet(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        const COLUMNS: usize = 2;
        let t = self.theme.clone();
        // the keymap's commands, then the keys that never change
        let map = &self.settings.keymap;
        let mut groups: Vec<SheetGroup> = Vec::new();
        for c in &COMMANDS {
            let keys: Vec<String> = map.keys(c.id).iter().map(keymap::text).collect();
            if keys.is_empty() {
                continue;
            }
            match groups.last_mut() {
                Some((g, list)) if *g == c.group => list.push((keys, c.label)),
                _ => groups.push((c.group, vec![(keys, c.label)])),
            }
        }
        for (group, list) in keymap::FIXED {
            groups.push((group, list.iter().map(|(k, what)| (vec![k.to_string()], *what)).collect()));
        }
        let title = tr!("Keyboard shortcuts · {} layout", map.preset.name());
        let width = (ctx.content_rect().width() - 48.0).min(720.0);
        let height = ctx.content_rect().height() - 140.0;
        let ((), close) = kit::modal(ctx, &t, "shortcuts", &title, width, |ui| {
            // a short window scrolls the sheet
            // (as tall as it needs up to that: a modal only knows last frame's size, so it can't grow into it)
            egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).auto_shrink([false, true]).show(ui, |ui| {
                ui.columns(COLUMNS, |cols| {
                    // fill the columns in order, each about as long as the others
                    let total: usize = groups.iter().map(|g| g.1.len() + 2).sum();
                    let (mut col, mut lines) = (0, 0);
                    for (group, list) in &groups {
                        if lines > total * (col + 1) / COLUMNS && col < COLUMNS - 1 {
                            col += 1;
                        }
                        lines += list.len() + 2;
                        let ui = &mut cols[col];
                        kit::caps(ui, &t, &i18n::t(group).to_uppercase());
                        for (keys, what) in list {
                            ui.horizontal(|ui| {
                                for (i, k) in keys.iter().enumerate() {
                                    if i > 0 {
                                        ui.label(RichText::new("/").small().color(t.fg_muted));
                                    }
                                    kbd(ui, &t, k);
                                }
                                ui.add(egui::Label::new(RichText::new(i18n::t(what)).color(t.fg_text)).truncate());
                            });
                        }
                        ui.add_space(t.pad);
                    }
                })
            });
        });
        if close {
            self.show_help = false;
        }
    }

    // ---- command palette ----------------------------------------------------

    pub fn open_palette(&mut self) {
        self.palette = Some(crate::Palette::default());
    }

    pub fn palette_ui(&mut self, ctx: &egui::Context) {
        let Some(pal) = &mut self.palette else { return };
        let t = self.theme.clone();
        let session = &self.session;
        let map = &self.settings.keymap;
        let query = pal.query.to_lowercase();
        let fits = |text: &str| query.split_whitespace().all(|w| text.to_lowercase().contains(w));
        // the app's commands, then operators: (label, note, keys, ready, what it runs)
        let mut matches: Vec<(&str, &str, String, bool, &'static str)> =
            COMMANDS.iter().filter(|c| c.id.starts_with("app.") && c.id != "app.palette" && fits(&format!("{} {} {}", c.label, i18n::t(c.label), c.id))).map(|c| (i18n::t(c.label), i18n::t(c.group), map.label(c.id), true, c.id)).collect();
        matches.extend(OPERATORS.iter().filter(|o| fits(&format!("{} {} {}", o.label, i18n::t(o.label), o.id))).map(|o| {
            let note = if o.example == "{}" { i18n::t(o.description) } else { tr!("needs input: use the UI or a script") };
            (i18n::t(o.label), note, map.label(o.id), o.example == "{}" && (o.poll)(session), o.id)
        }));
        let mut chosen = None;
        let (_, close) = kit::modal_flush(ctx, &t, "palette", tr!("Commands"), 560.0, |ui| {
            // the search on a padded band, the list edge to edge under a divider: its scroll
            // bar on the modal's right edge
            let r = egui::Frame::new().inner_margin(t.pad).show(ui, |ui| ui.add(kit::field(&mut pal.query).hint_text(format!("{}  {}", icons::MAGNIFYING_GLASS, tr!("Search commands…"))).desired_width(f32::INFINITY))).inner;
            r.request_focus();
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(line.x_range(), line.top() - 0.5, Stroke::new(1.0, t.line));
            let (up, down, enter) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter)));
            let last = matches.len().saturating_sub(1);
            pal.selected = if down {
                (pal.selected + 1).min(last)
            } else if up {
                pal.selected.saturating_sub(1)
            } else {
                pal.selected.min(last)
            };
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, (label, note, keys, ready, id)) in matches.iter().enumerate() {
                    let r = kit::row(ui, &t, Id::unique(("palette-row", id)), i == pal.selected);
                    let color = if *ready { t.fg_text } else { t.fg_muted };
                    let mut painter = ui.painter_at(r.rect);
                    painter.multiply_opacity(kit::row_alpha(ui, &r));
                    let name = painter.text(r.rect.left_center() + vec2(t.pad, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(13.0), color);
                    let keys_rect = painter.text(r.rect.right_center() - vec2(t.pad, 0.0), Align2::RIGHT_CENTER, keys, Theme::mono(11.0), t.fg_text);
                    let right = if keys.is_empty() { r.rect.right() - t.pad } else { keys_rect.left() - 2.0 * t.pad };
                    let room = right - name.right() - 2.0 * t.pad;
                    kit::fit_text(&painter, pos2(right, r.rect.center().y), Align2::RIGHT_CENTER, *note, FontId::proportional(11.0), t.fg_muted, room);
                    if r.clicked() && *ready {
                        chosen = Some(*id);
                    }
                    if i == pal.selected && (up || down) {
                        r.scroll_to_me(None);
                    }
                }
            });
            let foot = ui.available_rect_before_wrap();
            ui.painter().hline(foot.x_range(), foot.top() - 0.5, Stroke::new(1.0, t.line));
            egui::Frame::new().inner_margin(egui::Margin::symmetric(t.pad as i8, (t.gap * 1.5) as i8)).show(ui, |ui| {
                let found = if matches.is_empty() { tr!("Nothing found").to_string() } else { tr!("{} commands", matches.len()) };
                ui.label(RichText::new(tr!("{} · ↑ ↓ choose · Enter runs · Esc closes", found)).small().color(t.fg_muted));
            });
            if enter {
                chosen = matches.get(pal.selected).filter(|m| m.3).map(|m| m.4);
            }
        });
        if close || chosen.is_some() {
            self.palette = None;
        }
        if let Some(id) = chosen {
            self.command(id);
        }
    }

    // ---- rename dialog --------------------------------------------------------

    pub fn rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(r) = &mut self.rename else { return };
        let t = self.theme.clone();
        let title = match (&r.node, r.macro_index) {
            (Some(_), _) => tr!("Rename node"),
            (None, Some(_)) => tr!("Rename macro"),
            (None, None) => tr!("Rename scene"),
        };
        let (confirm, close) = kit::modal(ctx, &t, "rename", title, 340.0, |ui| {
            let edit = ui.add(kit::field(&mut r.text).desired_width(f32::INFINITY));
            if !r.focused {
                edit.request_focus();
                r.focused = true;
            }
            let mut confirm = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal(|ui| {
                confirm |= kit::button(ui, &t, Some(icons::CHECK), tr!("Rename"), true).clicked();
                ui.label(RichText::new(tr!("Enter to confirm · Esc to cancel")).small().color(t.fg_muted));
            });
            confirm
        });
        if confirm && let Some(r) = self.rename.take() {
            let name = r.text.trim().to_string();
            match (name.is_empty(), r.node, r.macro_index) {
                // an empty macro name is its number again
                (_, None, Some(index)) => self.run("macro.rename", json!({ "scene": r.scene, "index": index, "name": name })),
                (true, ..) => {}
                (false, Some(node), _) => self.run("node.rename", json!({ "scene": r.scene, "node": node, "name": name })),
                (false, None, None) => self.run("scene.rename", json!({ "scene": r.scene, "name": name })),
            }
        } else if close {
            self.rename = None;
        }
    }
}

/// A display as people say it: Windows calls them `\\.\DISPLAY1`.
fn display_name(name: &str) -> String {
    name.strip_prefix(r"\\.\DISPLAY").map_or_else(|| name.to_string(), |n| tr!("Display {}", n))
}

/// The inputs to listen to: the computer's sound, the default input, each device, each app that
/// uses audio. Returns the one picked, and whether the way to the permission was asked for.
/// What the audio input does, in a few words and in full: the audio chip's text and its tip.
fn audio_words(state: &AudioState, current: Option<&Input>) -> (String, String) {
    let app = match current {
        Some(Input::App { name, .. }) => Some(name.clone()),
        _ => None,
    };
    match (&state.hearing, &app) {
        (Hearing::Failed(e), Some(name)) => (tr!("Waiting for {}", name), e.clone()),
        (Hearing::Nothing, Some(name)) => (tr!("{} is silent", name), tr!("Nothing plays in {}. If something does: macOS System Settings → Privacy & Security → Screen & System Audio Recording, allow Velo, then reopen it.", name)),
        (hearing, _) => match hearing {
            Hearing::Live => (state.name.clone(), tr!("Listening to {}", state.name)),
            Hearing::Waiting if cfg!(target_os = "macos") => (tr!("Waiting for permission…").to_string(), tr!("Waiting for the input to start (on macOS, for your permission)").to_string()),
            Hearing::Nothing if cfg!(target_os = "macos") => (
                // short enough to fit a narrow bar: the tip and the menu's first item say how
                tr!("No sound: allow Velo").to_string(),
                tr!("Only silence arrives. macOS: System Settings → Privacy & Security → Screen & System Audio Recording (or Microphone): allow Velo, then reopen it.").to_string(),
            ),
            // elsewhere no permission holds it: the computer's sound (WASAPI loopback) sends
            // nothing at all while nothing plays
            // the input's name stays (the empty ring says it's silent): the bar doesn't jump when the music starts
            Hearing::Waiting | Hearing::Nothing => (if state.name.is_empty() { tr!("Silence").to_string() } else { state.name.clone() }, tr!("Nothing arrives yet: play something, or pick another input").to_string()),
            Hearing::Failed(e) => (tr!("No audio input").to_string(), e.clone()),
        },
    }
}

fn audio_menu(ui: &mut Ui, t: &Theme, state: &AudioState, current: Option<&Input>) -> (Option<Input>, bool) {
    let (mut pick, mut help) = (None, false);
    // nothing arrives: the way to the permission first (macOS: the one that holds it)
    if cfg!(target_os = "macos") && matches!(state.hearing, Hearing::Nothing | Hearing::Waiting) {
        help = ui.button(format!("{}  {}", icons::INFO, tr!("Let Velo hear the computer…"))).clicked();
        ui.separator();
    }
    let mut inputs: Vec<(&str, Input, String)> = vec![("", Input::Computer, format!("{}  {}", icons::SPEAKER_HIGH, tr!("Computer audio"))), ("", Input::Default, format!("{}  {}", icons::MICROPHONE, tr!("Default input")))];
    inputs.extend(state.devices.iter().map(|name| (tr!("DEVICES"), Input::Named(name.clone()), format!("{}  {name}", icons::MICROPHONE))));
    // one app's sound (macOS 14.2+): the apps that use audio, the one picked even while it's closed
    let mut apps: Vec<(String, String, bool)> = state.apps.iter().map(|a| (a.id.clone(), a.name.clone(), a.playing)).collect();
    if let Some(Input::App { id, name }) = current
        && !apps.iter().any(|a| &a.0 == id)
    {
        apps.push((id.clone(), name.clone(), false));
    }
    inputs.extend(apps.into_iter().map(|(id, name, playing)| (tr!("ONE APP"), Input::App { id, name: name.clone() }, format!("{}  {}", icons::APP_WINDOW, if playing { tr!("{} · playing", name) } else { name }))));
    let items: Vec<(&str, &str)> = inputs.iter().map(|(group, _, label)| (*group, label.as_str())).collect();
    if let Some(i) = kit::choose(ui, t, &items, inputs.iter().position(|(_, input, _)| current == Some(input))) {
        pick = Some(inputs.swap_remove(i).1);
    }
    (pick, help)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_long_ago_is_said_in_the_largest_unit_that_fits() {
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(150), "2 min ago");
        assert_eq!(ago(2 * 3600 + 5), "2 h ago");
        assert_eq!(ago(30 * 3600), "yesterday");
        assert_eq!(ago(3 * 86_400), "3 days ago");
    }
}
