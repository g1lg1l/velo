//! Velo: an audio-first instrument for live visuals.
//! From Ravune (bd10814), trimmed for Velo.
//! Every edit goes through `velo_core` operators, so keymaps, the command palette, undo and
//! (later) MIDI, scripts and agents share one path.

// release builds are a window, not a console program: no terminal opens beside Velo on
// Windows (debug builds keep it for their logs)
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
mod i18n;

mod agent;
mod ai;
mod canvas;
mod chat;
mod code;
mod files;
mod host;
mod icons;
mod keymap;
mod kit;
#[cfg(feature = "link")]
mod link;
mod live;
mod markdown;
mod midi;
mod orb;
mod osc;
mod shell;
mod specimen;
mod templates;
mod theme;
mod update;
mod usage;
mod widgets;

use shell::Tone;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use eframe::egui::{self, Event, Frame, Id, Response, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use theme::Theme;
use velo_core::{CANVAS, Category, Curve, LAUNCH_BEATS, Launch, Layer, LayerBlend, Mapping, ParamInfo, Project, Quantize, SOURCES, Scene, Session, Target, Transition, VARIATIONS, node_kind};
use velo_engine::Engine;
use widgets::ModView;

/// Our own event loop (spike S1): the engine starts first, on its own GPU device, then eframe
/// runs inside a host that also owns the output window.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    let engine = match Engine::start(Some(Box::new(event_loop.owned_display_handle()))) {
        Ok(engine) => Some(Rc::new(engine)),
        Err(e) => {
            eprintln!("velo: no GPU for the engine, nothing will render: {e}");
            None
        }
    };
    let shared = Rc::new(RefCell::new(host::Shared::default()));
    let icon = eframe::icon_data::from_png_bytes(widgets::ICON_256).expect("assets/icons/png/velo-256.png is a PNG");
    let viewport = egui::ViewportBuilder::default().with_title("Velo").with_icon(icon).with_inner_size([1280.0, 800.0]).with_min_inner_size([900.0, 600.0]);
    // VELO_CAPTURE_TO: a window for screenshots that never gets in anyone's way: off-screen,
    // unfocused, off the taskbar, wherever the profile left it
    let capture = std::env::var_os("VELO_CAPTURE_TO").is_some();
    // (VELO_CAPTURE_SIZE=w,h: another size than 1440 × 900)
    let size = std::env::var("VELO_CAPTURE_SIZE").ok().and_then(|v| v.split_once(',').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?]))).unwrap_or([1440.0, 900.0]);
    let viewport = if capture { viewport.with_inner_size(size).with_position([-6000.0, 0.0]).with_active(false).with_taskbar(false) } else { viewport };
    let options = eframe::NativeOptions {
        viewport,
        persist_window: !capture,
        // a separate profile keeps its window and settings apart too
        persistence_path: std::env::var_os("VELO_DATA").and_then(|_| files::data_dir()).map(|d| d.join("app.ron")),
        ..Default::default()
    };
    let (ui_engine, ui_shared) = (engine.clone(), shared.clone());
    let app = eframe::create_native("Velo", options, Box::new(move |cc| Ok(Box::new(App::new(cc, ui_engine.map(|e| live::Live::new(e, ui_shared)))))), &event_loop);
    event_loop.run_app(&mut host::Host::new(app, engine, shared))?;
    Ok(())
}

/// The autopilot's next scene among those that take turns (not a layer, not left out): the one
/// after `now`, or with `random` (0..1) any but `now`. None with fewer than two.
fn next_turn(scenes: &[Scene], now: Option<&str>, random: Option<f64>) -> Option<String> {
    let turns: Vec<&Scene> = scenes.iter().filter(|s| s.layer == Layer::Scene && !s.manual).collect();
    if turns.len() < 2 {
        return None;
    }
    let at = now.and_then(|id| turns.iter().position(|s| s.id == id));
    let pick = match (random, at) {
        (Some(r), Some(at)) => (at + 1 + (r * (turns.len() - 1) as f64) as usize) % turns.len(),
        (Some(r), None) => (r * turns.len() as f64) as usize % turns.len(),
        (None, Some(at)) => (at + 1) % turns.len(),
        (None, None) => 0,
    };
    Some(turns[pick].id.clone())
}

/// The usual canvas sizes, for the Canvas section's menu.
const RESOLUTIONS: [(&str, u32, u32); 9] =
    [("HD", 1280, 720), ("Full HD", 1920, 1080), ("QHD", 2560, 1440), ("4K UHD", 3840, 2160), ("Ultrawide", 2560, 1080), ("4:3", 1024, 768), ("Square", 1080, 1080), ("Vertical", 1080, 1920), ("LED wall", 1536, 768)];

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    reduce_motion: bool,
    /// The theme in use.
    theme: Theme,
    /// Themes the user imported.
    themes: Vec<Theme>,
    keymap: keymap::Keymap,
    language: i18n::Lang,
    panels: Panels,
    /// What focus mode (Cmd+\) hid, to bring back.
    panels_before_focus: Option<Panels>,
    /// The floating preview card is hidden (P).
    hide_preview: bool,
    /// The preview fills the canvas behind the nodes.
    preview_background: bool,
    /// The canvas split in two: the picture beside (or under) the nodes, and its share of
    /// the canvas (0: the default).
    split: Split,
    split_share: f32,
    /// The inspector in a window of its own over everything, instead of docked.
    inspector_floating: bool,
    /// The audio input, remembered per machine.
    audio: live::Input,
    /// Don't ask GitHub for a newer Velo at start (asking is the default).
    no_update_check: bool,
    /// What agents (MCP clients) may do.
    agent: agent::Prefs,
    /// Which AI the AI panel uses, and the local model's endpoint.
    ai: ai::Prefs,
    /// OSC in: on or off, its port.
    osc: osc::Prefs,
    /// Auto-BPM: the beat clock follows the music's tempo while it's sure of it.
    auto_bpm: bool,
    /// Ableton Link on (in builds with it).
    link: bool,
    /// The first-run demo was shown (it opens once, then Project → Open the demo).
    demo_seen: bool,
    /// The frame-time HUD over the canvas (click the fps in the top bar).
    hud: bool,
    /// The scenes as a list of slim rows (many fit) instead of cards with their pictures large.
    scenes_compact: bool,
    /// The outside code editor for shader files: an app on macOS ("Visual Studio Code"), a
    /// command elsewhere ("code"); empty, the system's app for .wgsl files.
    editor: String,
    /// Projects opened or saved, the latest first (Project → Open Recent).
    recent: Vec<std::path::PathBuf>,
    /// How the autopilot plays (it's off at every start).
    autopilot: Autopilot,
}

/// The autopilot: another scene launched every so many bars or seconds, and on a drop if asked.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Autopilot {
    #[serde(skip)]
    on: bool,
    /// Bars between launches: 4, 8, 16 or 32.
    bars: u32,
    /// Seconds between launches instead of bars (the clock's, not the music's).
    timed: bool,
    seconds: u32,
    /// Scenes picked at random (never the one out), else in order.
    random: bool,
    /// A drop (the music back at full after a quieter while) launches the next one too.
    drops: bool,
}

impl Default for Autopilot {
    fn default() -> Self {
        Self { on: false, bars: 8, timed: false, seconds: 60, random: false, drops: true }
    }
}

/// The autopilot's memory between frames: the next turn, and the bass followed fast, slow and
/// at its strongest lately (a drop is the fast one well over the slow one).
#[derive(Default)]
struct Pilot {
    /// The next turn: in beats, or in seconds when it's timed.
    next: Option<f64>,
    timed: bool,
    /// The bass as it is now: followed in 0.4 s, so a kick's pulse is evened out.
    fast: f32,
    /// The bass as it has been: up in 1.5 s, down in 6, so it stays near the top of steady
    /// kicks and has sunk by the end of a breakdown.
    slow: f32,
    /// The strongest `fast` of the last half minute: what a drop is measured against.
    strong: f32,
    last_drop: f64,
}

/// How far over `slow` the bass must come back for a drop, as a part of `strong`. Measured on
/// the test loop through the analyzer: steady kicks stay under 0, the bass back after a bar or
/// two reaches 0.1, after 4 bars or more 0.2 to 0.5 (the least after a long breakdown that
/// keeps some bass, which the analyzer's auto-gain has lifted by then).
const DROP_RISE: f32 = 0.15;
/// The least `strong` a drop is measured against: under it the input is too quiet to tell a
/// drop from hiss.
const DROP_QUIET: f32 = 0.05;

impl Pilot {
    /// The bass this frame, `dt` seconds on: whether the music just dropped (the bass back
    /// after a while without it, not within 4 bars of the last). The level can't say: in a
    /// breakdown the pads are as loud as the kick was. Measured against how strong the bass
    /// has been lately, so a low input gain drops as a high one does.
    fn dropped(&mut self, bass: f32, dt: f32, beats: f64) -> bool {
        let toward = |from: f32, seconds: f32| from + (bass - from) * (dt / seconds).min(1.0);
        self.fast = toward(self.fast, 0.4);
        self.slow = toward(self.slow, if bass > self.slow { 1.5 } else { 6.0 });
        self.strong = self.fast.max(self.strong * (1.0 - (dt / 30.0).min(1.0)));
        // either side of the last: a clock set back must not wait for its old count
        let drop = self.fast - self.slow > DROP_RISE * self.strong.max(DROP_QUIET) && (beats - self.last_drop).abs() > 16.0;
        if drop {
            self.last_drop = beats;
        }
        drop
    }
}

/// Where the bass is among [`SOURCES`], as `Live::sources` gives them.
const BASS: usize = 1;

/// Seconds as a short duration: "45 s", "3 min", "1 min 30 s".
fn clock(seconds: u32) -> String {
    match (seconds / 60, seconds % 60) {
        (0, s) => format!("{s} s"),
        (m, 0) => format!("{m} min"),
        (m, s) => format!("{m} min {s} s"),
    }
}

/// Which panels around the canvas are open.
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Panels {
    scenes: bool,
    inspector: bool,
    sources: bool,
    /// The AI (M5), closed until asked for.
    chat: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self { scenes: true, inspector: true, sources: true, chat: false }
    }
}

impl Panels {
    const NONE: Panels = Panels { scenes: false, inspector: false, sources: false, chat: false };
}

impl Settings {
    /// The theme as drawn: the chosen one, with motion off if asked.
    fn effective_theme(&self) -> Theme {
        // a built-in is taken by name, so its updates reach people who picked it before
        let mut theme = Theme::builtins().into_iter().find(|b| b.name == self.theme.name).unwrap_or_else(|| self.theme.clone());
        if self.reduce_motion {
            theme.motion = 0.0;
        }
        theme
    }
}

#[derive(Default)]
struct Palette {
    query: String,
    selected: usize,
}

/// Where the picture goes when the canvas is split (Cmd+Shift+P cycles through them).
#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
enum Split {
    #[default]
    Off,
    Right,
    Bottom,
}

struct App {
    /// When the engine was last asked to start again.
    restarted: Option<Instant>,
    pilot: Pilot,
    session: Session,
    settings: Settings,
    theme: Theme,
    canvas: canvas::State,
    /// The center between the panels, as it was last frame (the split canvas' parts come from it).
    center_rect: Option<egui::Rect>,
    palette: Option<Palette>,
    show_settings: bool,
    show_help: bool,
    rename: Option<shell::Renaming>,
    /// The Shader node whose code is open in the editor.
    editing: Option<String>,
    /// Shader files watched for an outside editor's saves.
    watch: files::Watch,
    /// The MCP server for agents, its log.
    agent: agent::Agent,
    /// The AI panel's conversation.
    chat: chat::Chat,
    /// The OSC port, while it's on.
    osc: Option<osc::Osc>,
    /// Auto-BPM's last estimate taken, the one before, and when it last set the clock.
    auto: shell::Auto,
    /// The render thread's last frame times (ms), for the HUD's graph.
    frame_times: std::collections::VecDeque<f32>,
    /// Help for whatever the mouse is over (collected this frame, shown next).
    info: String,
    info_next: String,
    toast: Option<shell::Toast>,
    updates: update::Updates,
    /// Every widget in its states (spike S2), when open.
    specimen: Option<specimen::Specimen>,
    cpu: Cpu,
    usage: usage::Usage,
    /// The splash while Velo starts: when it began to fade, once all's ready; none once gone.
    splash: Option<Option<f64>>,
    /// When the splash came up (set on its first frame): what its waits count from.
    splash_from: Option<f64>,
    /// The engine and the audio input; none in headless tests (or without a GPU).
    live: Option<live::Live>,
    /// Controllers: what came in, and MIDI learn.
    midi: midi::Midi,
    learn: midi::Learn,
    /// An empty macro clicked, and its scene: the next value touched in the inspector there is
    /// the one it drives.
    assigning: Option<(String, usize)>,
    /// Space went down at this time; true once the pointer was pressed meanwhile (a pan).
    space: Option<(f64, bool)>,
    /// `VELO_DEMO_SCENES`: a scene to select once the first one is live, and whether to launch it.
    demo_drop: Option<(String, bool)>,
    /// The start screen is up: the recent projects, a new one, one from a file, the demo.
    start: bool,
    /// When the last UI frame started: the next one never comes sooner than the display needs.
    painted: Instant,
    /// What the last frame waited for that, in seconds: not the UI's cost.
    waited: f32,
    // files
    file: Option<PathBuf>,
    saved: Project,
    autosaved: Project,
    last_autosave: f64,
    pending: Option<files::Pending>,
    /// Quitting with the answer given: Some(false) until the close is asked for, then Some(true).
    quitting: Option<bool>,
    /// The restore dialog closed without an answer: offered again next time.
    recovery_later: bool,
    recovery: Option<Project>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, live: Option<live::Live>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let settings: Settings = cc.storage.and_then(|s| eframe::get_value(s, "settings")).unwrap_or_default();
        i18n::set(settings.language);
        let theme = settings.effective_theme();
        theme.apply(&cc.egui_ctx);
        // Cmd/Ctrl+= and - zoom the canvas, as in Figma: not egui's whole-UI scale
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let mut app = Self {
            session: Session::default(),
            settings,
            theme,
            canvas: canvas::State::default(),
            center_rect: None,
            palette: None,
            show_settings: false,
            show_help: false,
            rename: None,
            editing: None,
            watch: files::Watch::default(),
            agent: agent::Agent::default(),
            chat: chat::Chat::default(),
            osc: None,
            auto: shell::Auto::default(),
            frame_times: std::collections::VecDeque::new(),
            info: String::new(),
            info_next: String::new(),
            toast: None,
            updates: update::Updates::default(),
            specimen: specimen::Specimen::from_env(),
            cpu: Cpu::default(),
            usage: usage::Usage::new(),
            splash: (std::env::var_os("VELO_CAPTURE_TO").is_none() || std::env::var_os("VELO_DEMO_SPLASH").is_some()).then_some(None),
            splash_from: None,
            live,
            restarted: None,
            pilot: Pilot::default(),
            midi: midi::Midi::start(&cc.egui_ctx),
            learn: midi::Learn::default(),
            assigning: None,
            space: None,
            demo_drop: None,
            start: false,
            painted: Instant::now(),
            waited: 0.0,
            file: None,
            saved: Project::default(),
            autosaved: Project::default(),
            last_autosave: 0.0,
            pending: None,
            quitting: None,
            recovery_later: false,
            recovery: files::take_recovery(),
        };
        // opened by itself, Velo always starts on the start screen, unsaved work from last time
        // offered there (a project loading below, the first run's demo too, closes it); a
        // screenshot run only with VELO_DEMO_START
        let shots = std::env::var_os("VELO_CAPTURE_TO").is_some();
        app.start = std::env::args_os().nth(1).is_none() && (!shots || std::env::var_os("VELO_DEMO_START").is_some());
        // `velo show.velo` (also what double-clicking a project file does)
        if let Some(path) = std::env::args_os().nth(1) {
            app.request(files::Pending::Open(path.into()));
        }
        // the first time Velo opens: the demo, live, so the first five minutes start with
        // something moving
        if !app.settings.demo_seen && std::env::args_os().nth(1).is_none() && app.recovery.is_none() && std::env::var_os("VELO_DEMO_GRAPH").is_none() {
            app.settings.demo_seen = true;
            app.request(files::Pending::Demo);
        }
        // VELO_DEMO_SCENE=n: scene n selected, so it's the one that goes live; past the demo's one
        // scene, template n added (selected) for it
        if let Some(n) = std::env::var("VELO_DEMO_SCENE").ok().and_then(|n| n.parse::<usize>().ok()) {
            match app.session.project.scenes.get(n.saturating_sub(1)) {
                Some(scene) => app.session.active_scene = Some(scene.id.clone()),
                None => {
                    if let Some(template) = templates::TEMPLATES.get(n.saturating_sub(1)) {
                        let _ = templates::add(&mut app.session, template, None);
                        app.saved = app.session.project.clone();
                    }
                }
            }
        }
        // VELO_DEMO_FIND=words: the canvas' search open with them typed (screenshots)
        if let Ok(words) = std::env::var("VELO_DEMO_FIND") {
            app.canvas.find = Some(canvas::Find::with(&words));
        }
        // VELO_DEMO_EDIT=n: scene n selected once the first one is live: edited, not live (tests)
        if let Some(n) = std::env::var("VELO_DEMO_EDIT").ok().and_then(|n| n.parse::<usize>().ok())
            && let Some(scene) = app.session.project.scenes.get(n.saturating_sub(1))
        {
            app.demo_drop = Some((scene.id.clone(), false));
        }
        // VELO_DEMO_SHARE=spout|ndi: the program shared that way from the start (tests)
        if let (Ok(way), Some(live)) = (std::env::var("VELO_DEMO_SHARE"), &mut app.live) {
            live.share(Some(if way == "ndi" { velo_engine::share::Share::Ndi } else { velo_engine::share::Share::Spout }));
        }
        // VELO_DEMO_AUTOPILOT=bars|<n>s: the autopilot on, a scene every that many bars or seconds (tests)
        if let Ok(every) = std::env::var("VELO_DEMO_AUTOPILOT") {
            let a = app.settings.autopilot;
            app.settings.autopilot = match every.strip_suffix('s').map(str::parse) {
                Some(Ok(seconds)) => Autopilot { on: true, timed: true, seconds, ..a },
                _ => Autopilot { on: true, timed: false, bars: every.parse().unwrap_or(a.bars), ..a },
            };
        }
        // VELO_DEMO_AGENT: agents on (testing the MCP bridge end to end)
        if std::env::var_os("VELO_DEMO_AGENT").is_some() {
            app.settings.agent.enabled = true;
        }
        // VELO_DEMO_CHAT=prompt: the AI panel open, sending it (end-to-end tests)
        if let Ok(prompt) = std::env::var("VELO_DEMO_CHAT") {
            app.settings.panels.chat = true;
            app.chat.draft = prompt;
            app.chat.send_now = !app.chat.draft.is_empty();
        }
        // VELO_DEMO_PROVIDER=claude|codex|local[:<model>]: which AI the panel uses, with which
        // model (tests)
        if let Ok(p) = std::env::var("VELO_DEMO_PROVIDER") {
            let (name, model) = p.split_once(':').unwrap_or((&p, ""));
            let provider = match name {
                "local" => ai::Provider::Local,
                "codex" => ai::Provider::Codex,
                _ => ai::Provider::ClaudeCode,
            };
            app.settings.ai.provider = provider;
            *provider.model(&mut app.settings.ai) = model.to_string();
        }
        // VELO_DEMO_ATTACH=<picture>[;<picture>…]: attached to the AI panel's next message
        if let Ok(files) = std::env::var("VELO_DEMO_ATTACH") {
            app.settings.panels.chat = true;
            app.chat_attach_files(&files.split(';').map(PathBuf::from).collect::<Vec<_>>());
        }
        // VELO_DEMO_OSC=port: OSC on (end-to-end tests)
        if let Some(port) = std::env::var("VELO_DEMO_OSC").ok().and_then(|p| p.parse().ok()) {
            app.settings.osc = osc::Prefs { enabled: true, port };
        }
        // VELO_DEMO_AUTO_BPM: auto-BPM on (end-to-end tests)
        if std::env::var_os("VELO_DEMO_AUTO_BPM").is_some() {
            app.settings.auto_bpm = true;
        }
        // VELO_DEMO_ISF=file.fs: imported into a scene of its own, wired out (end-to-end tests)
        if let Some(path) = std::env::var_os("VELO_DEMO_ISF").map(PathBuf::from) {
            app.run("scene.add", json!({ "name": "ISF" }));
            app.import_isf(&path);
            if let Some(shader) = app.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) {
                app.run("node.add", json!({ "kind": "velo.output", "pos": [240.0, 0.0] }));
                if let Some(out) = app.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) {
                    app.run("edge.connect", json!({ "from": [shader, 0], "to": [out, 0] }));
                }
            }
        }
        // VELO_DEMO_LFO: an LFO in the selected scene, moving its first effect's first parameter
        if std::env::var_os("VELO_DEMO_LFO").is_some()
            && let Some(scene) = app.session.active()
            && let Some(target) = scene.nodes.iter().find(|n| matches!(n.kind.as_str(), "velo.feedback" | "velo.kaleido" | "velo.shapes")).map(|n| n.id.clone())
        {
            app.run("node.add", json!({ "kind": velo_core::LFO, "pos": [0.0, 260.0] }));
            if let Some(lfo) = app.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) {
                app.run("node.set_param", json!({ "node": lfo, "param": "beats", "value": 2.0 }));
                app.run("mapping.add", json!({ "node": target, "param": "zoom", "source": lfo }));
                app.canvas.selected = vec![lfo];
            }
        }
        // VELO_DEMO_IMAGE=<file or folder>: dropped on the selected scene as an Image node
        if let Some(path) = std::env::var_os("VELO_DEMO_IMAGE") {
            app.import_media(std::path::Path::new(&path));
            // and out: into the Output node
            let ids = app.session.active().and_then(|s| Some((s.nodes.last()?.id.clone(), s.nodes.iter().find(|n| n.kind == "velo.output")?.id.clone())));
            if let Some((image, out)) = ids {
                app.run("edge.connect", json!({ "from": [image, 0], "to": [out, 0] }));
            }
        }
        // VELO_DEMO_NODE=<kind>: a node of that kind put before the Output, selected
        if let Ok(kind) = std::env::var("VELO_DEMO_NODE")
            && let Some(scene) = app.session.active()
            && let Some(out) = scene.nodes.iter().find(|n| n.kind == "velo.output").map(|n| n.id.clone())
        {
            let feeding = scene.wire_into(&out, 0).map(|e| e.from.clone());
            // (VELO_DEMO_PRESET=slug: a Shader or Shader FX running that preset)
            let preset = std::env::var("VELO_DEMO_PRESET").ok().and_then(|slug| velo_engine::PRESETS.iter().chain(&velo_engine::EFFECT_PRESETS).find(|p| p.0 == slug)).map(|p| p.1);
            app.run("node.add", json!({ "kind": kind, "pos": [0.0, 220.0], "code": preset.or_else(|| velo_engine::template(&kind)) }));
            if let Some(new) = app.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()) {
                // only the wires that fit (a signal node takes no picture, but a Probe does)
                let fits = |app: &App, from: (&str, u8), to: (&str, u8)| app.session.active().is_some_and(|s| s.can_connect(from, to).is_ok());
                if let Some((from, port)) = feeding.filter(|(from, port)| fits(&app, (from, *port), (&new, 0))) {
                    app.run("edge.connect", json!({ "from": [from, port], "to": [new, 0] }));
                }
                if fits(&app, (&new, 0), (&out, 0)) {
                    app.run("edge.connect", json!({ "from": [new, 0], "to": [out, 0] }));
                }
                app.canvas.selected = vec![new];
            }
        }
        // VELO_DEMO_PROPS: the selected node's parameters dropped down on the canvas
        if std::env::var_os("VELO_DEMO_PROPS").is_some() {
            app.canvas.props = app.canvas.selected.first().cloned();
        }
        // VELO_DEMO_SPLIT=right|bottom: the canvas split; VELO_DEMO_FLOAT: the inspector floating
        match std::env::var("VELO_DEMO_SPLIT").as_deref() {
            Ok("right") => app.settings.split = Split::Right,
            Ok("bottom") => app.settings.split = Split::Bottom,
            _ => {}
        }
        if std::env::var_os("VELO_DEMO_FLOAT").is_some() {
            app.settings.inspector_floating = true;
        }
        if std::env::var_os("VELO_DEMO_PREFS").is_some() {
            app.show_settings = true;
        }
        // VELO_DEMO_COMPACT: the scenes as the compact list (screenshots)
        if std::env::var_os("VELO_DEMO_COMPACT").is_some() {
            app.settings.scenes_compact = true;
        }
        // VELO_DEMO_HUD: the frame-time HUD on (screenshots)
        if std::env::var_os("VELO_DEMO_HUD").is_some() {
            app.settings.hud = true;
        }
        // dev and screenshots: a patch to look at (1) or a stress graph of about n nodes
        if let Some(n) = std::env::var("VELO_DEMO_GRAPH").ok().and_then(|v| v.parse().ok()) {
            let mut selected = canvas::demo(&mut app.session, n);
            // VELO_DEMO_MAPPING: Kick moves Kaleido's zoom, selected (a mapped slider to look at)
            if std::env::var_os("VELO_DEMO_MAPPING").is_some()
                && let Some(kaleido) = app.session.active().and_then(|s| s.nodes.iter().find(|n| n.kind == "velo.kaleido")).map(|n| n.id.clone())
            {
                let _ = app.session.run("mapping.add", json!({ "node": kaleido, "param": "zoom", "source": "kick" }));
                // VELO_DEMO_MAPPING=open: its link's popover open too
                if std::env::var("VELO_DEMO_MAPPING").as_deref() == Ok("open")
                    && let Some(source) = app.session.active().and_then(|s| s.node(&kaleido)).and_then(|n| n.mappings.first()).map(|m| m.source.clone())
                {
                    app.canvas.map = Some((kaleido.clone(), source));
                }
                selected = Some(kaleido);
            }
            // VELO_DEMO_SIGNAL: Audio In's bass through a Remap and the beat through an Envelope,
            // the louder of the two (a Math) moving Kaleido's zoom, the Math selected
            if std::env::var_os("VELO_DEMO_SIGNAL").is_some()
                && let Some(kaleido) = app.session.active().and_then(|s| s.nodes.iter().find(|n| n.kind == "velo.kaleido")).map(|n| n.id.clone())
            {
                let mut add = |kind: &str, x: f32, y: f32| {
                    let _ = app.session.run("node.add", json!({ "kind": kind, "pos": [x, y] }));
                    app.session.active().and_then(|s| s.nodes.last()).map(|n| n.id.clone()).unwrap_or_default()
                };
                let (audio, remap) = (add(velo_core::AUDIO, 0.0, 640.0), add(velo_core::REMAP, 260.0, 640.0));
                let (beat, envelope) = (add(velo_core::CLOCK, 0.0, 880.0), add(velo_core::ENVELOPE, 260.0, 880.0));
                let math = add(velo_core::MATH, 520.0, 730.0);
                for (from, port, to, input) in [(&audio, 1, &remap, 0), (&beat, 0, &envelope, 0), (&remap, 0, &math, 0), (&envelope, 0, &math, 1)] {
                    let _ = app.session.run("edge.connect", json!({ "from": [from, port], "to": [to, input] }));
                }
                let _ = app.session.run("node.set_param", json!({ "node": math, "param": "operation", "value": 4.0 }));
                let _ = app.session.run("mapping.add", json!({ "node": kaleido, "param": "zoom", "source": format!("{math}/0") }));
                let _ = app.session.run("node.set_icon", json!({ "node": kaleido, "icon": "🌀" }));
                let _ = app.session.run("scene.set_icon", json!({ "icon": "🔥" }));
                if std::env::var_os("VELO_DEMO_PROPS").is_some() {
                    app.canvas.props = Some(math.clone());
                }
                selected = Some(math);
            }
            // VELO_DEMO_MACROS: named macros driving the patch, variations, bindings; Kaleido
            // selected (its sliders show their macro), MIDI learn on and waiting for Macro 4
            if std::env::var_os("VELO_DEMO_MACROS").is_some()
                && let Some(kaleido) = canvas::demo_macros(&mut app.session)
            {
                selected = Some(kaleido);
                app.learn.on = true;
                app.learn.wait(Target::Macro(3));
            }
            // VELO_DEMO_SHADER=1|broken: a Shader node's scene, the node selected, its code open
            if let Ok(demo) = std::env::var("VELO_DEMO_SHADER") {
                selected = canvas::demo_shader(&mut app.session, demo == "broken");
                // VELO_DEMO_PRESET=slug: running that preset, its code closed
                let preset = std::env::var("VELO_DEMO_PRESET").ok().and_then(|slug| velo_engine::PRESETS.iter().chain(&velo_engine::EFFECT_PRESETS).find(|p| p.0 == slug));
                match (preset, &selected) {
                    (Some((_, code)), Some(node)) => {
                        let _ = app.session.run("node.set_code", json!({ "node": node, "code": code }));
                    }
                    _ => app.editing.clone_from(&selected),
                }
            }
            // VELO_DEMO_SCENES: a second scene, "Drop", selected while the first goes live; with
            // "armed" it's launched too (on 4 bars: a countdown to look at)
            if let Ok(demo) = std::env::var("VELO_DEMO_SCENES") {
                let intro = app.session.active_scene.clone();
                let _ = app.session.run("scene.rename", json!({ "name": "Intro" }));
                app.demo_drop = canvas::demo_drop(&mut app.session).map(|id| (id, demo == "armed"));
                // the selected scene goes live at start: the first one, then the second is selected
                app.session.active_scene = intro;
                selected = None;
            }
            // a fresh history: undo shouldn't take the demo apart node by node
            let active = app.session.active_scene.take();
            app.session = Session::with_project(app.session.project.clone());
            app.session.active_scene = active;
            app.canvas.selected = selected.into_iter().collect();
            // part of what was opened: closing doesn't ask to save it, or keep it for recovery
            app.saved = app.session.project.clone();
        }
        app.canvas.open_menu = std::env::var_os("VELO_OPEN_NODE_MENU").is_some();
        // the computer's or an app's sound: its tempo leads by default (nobody types a DJ's BPM)
        if matches!(app.settings.audio, live::Input::Computer | live::Input::App { .. }) {
            app.settings.auto_bpm = true;
        }
        // VELO_AUDIO=synth: the test loop instead of a device (tests, screenshots, no permission)
        if let Some(live) = &mut app.live {
            live.audio.listen(live::Audio::first_input(&app.settings.audio));
            live.set_link(app.settings.link);
        }
        app
    }

    // ---- operators ----------------------------------------------------------

    fn run(&mut self, id: &str, props: Value) {
        if let Err(e) = self.session.run(id, props) {
            self.say(Tone::Error, e);
        }
    }

    fn run_merge(&mut self, id: &str, props: Value, key: &str) {
        if let Err(e) = self.session.run_merge(id, props, key) {
            self.say(Tone::Error, e);
        }
    }

    /// A toast. Operators' errors arrive in English: they're shown in the interface
    /// language when the catalog has them.
    fn say(&mut self, tone: Tone, msg: impl Into<String>) {
        let msg = msg.into();
        self.toast = Some(shell::Toast::new(tone, i18n::lookup(&msg).map_or(msg, String::from)));
    }

    /// A toast with an Undo on it: `what` comes back in its `acted` when it's clicked.
    fn say_undo(&mut self, msg: impl Into<String>, what: &'static str) {
        self.toast = Some(shell::Toast::new(Tone::Done, msg.into()).with_action(tr!("Undo"), what));
    }

    /// "Label (Ctrl K)" with whatever the keymap binds to `command` right now.
    fn tip(&self, label: &str, command: &str) -> String {
        match self.settings.keymap.label(command) {
            keys if keys.is_empty() => label.to_string(),
            keys => format!("{label} ({keys})"),
        }
    }

    /// What a control does, in a tooltip while the pointer rests on it.
    fn hint(&self, r: &Response, text: impl Into<String>) {
        r.clone().on_hover_text(text.into());
    }

    /// Runs a keymap command (`keymap::COMMANDS`): app actions here, the rest are operators.
    fn command(&mut self, id: &str) {
        match id {
            "app.palette" => self.open_palette(),
            "app.new" => self.request(files::Pending::New),
            "app.open" => self.open_dialog(),
            "app.save" => {
                self.save();
            }
            "app.save_as" => {
                self.save_as();
            }
            "app.import" => self.import_dialog(),
            "app.export" => self.export_dialog(None),
            "app.export_scene" => {
                if let Some(id) = self.session.active_scene.clone() {
                    self.export_dialog(Some(&id));
                }
            }
            "app.preferences" => self.show_settings = !self.show_settings,
            "app.demo" => self.request(files::Pending::Demo),
            "canvas.import_isf" => self.import_isf_dialog(),
            "app.hud" => self.settings.hud ^= true,
            "app.shortcuts" => self.show_help = !self.show_help,
            "app.specimen" => self.specimen = self.specimen.is_none().then(specimen::Specimen::default),
            "app.rename" => {
                if let Some(scene) = self.session.active() {
                    self.rename = Some(shell::Renaming::new(&scene.id, &scene.name));
                }
            }
            "app.toggle_scenes" => self.settings.panels.scenes ^= true,
            "app.toggle_inspector" => self.settings.panels.inspector ^= true,
            "app.toggle_sources" => self.settings.panels.sources ^= true,
            "app.chat" => {
                self.settings.panels.chat ^= true;
                self.chat.focus = self.settings.panels.chat;
            }
            "app.focus" => {
                let p = &mut self.settings.panels;
                if *p == Panels::NONE {
                    *p = self.settings.panels_before_focus.take().unwrap_or_default();
                } else {
                    self.settings.panels_before_focus = Some(*p);
                    *p = Panels::NONE;
                }
            }
            "stage.preview" => self.settings.hide_preview ^= true,
            "app.split" => {
                self.settings.split = match self.settings.split {
                    Split::Off => Split::Right,
                    Split::Right => Split::Bottom,
                    Split::Bottom => Split::Off,
                };
                // back from a split, the picture comes back in its card (picking the split in
                // the preview menu had hidden it)
                if self.settings.split == Split::Off {
                    self.settings.hide_preview = false;
                }
            }
            "app.float_inspector" => {
                self.settings.inspector_floating ^= true;
                self.settings.panels.inspector = true;
            }
            "stage.learn" => {
                let on = !self.learn.on;
                self.learn.stop();
                self.learn.on = on;
            }
            "stage.tap" => {
                // a hand on the tempo locks it, as the tempo menu's Tap does
                self.settings.auto_bpm = false;
                if let Some(live) = &self.live {
                    live.tap();
                }
            }
            "stage.blackout" => {
                if let Some(live) = &mut self.live {
                    live.set_blackout(!live.blackout);
                }
            }
            "stage.freeze" => {
                if let Some(live) = &mut self.live {
                    live.set_freeze(!live.frozen);
                }
            }
            "stage.pattern" => {
                if let Some(live) = &mut self.live {
                    live.set_pattern(!live.pattern);
                }
            }
            "stage.launch" => {
                if let Some(scene) = self.session.active_scene.clone() {
                    self.launch(&scene);
                }
            }
            "stage.auto" => {
                // back to playing by itself: the autopilot on, every layer on, and off a scene
                // launched by hand that the autopilot leaves alone
                self.settings.autopilot.on = true;
                if let Some(live) = &mut self.live {
                    live.off.clear();
                }
                if self.held_by_hand() {
                    self.turn(None);
                }
            }
            // a drop by hand, heard or not: the next scene at once, through white
            "stage.drop" => self.turn(Some(Launch { transition: Transition::Flash, duration: 1.0, quantize: Quantize::Off })),
            id if id.starts_with("stage.scene_") => {
                let n: usize = id["stage.scene_".len()..].parse().unwrap_or(0);
                if let Some(scene) = n.checked_sub(1).and_then(|i| self.session.project.scenes.get(i)).map(|s| s.id.clone()) {
                    self.launch(&scene);
                }
            }
            "canvas.add" => self.canvas.open_menu = true,
            "canvas.find" => self.canvas.find.get_or_insert_default().refocus(),
            "canvas.home" => self.canvas.zoom_to = Some(canvas::ZoomTo::Fit),
            "canvas.zoom_selection" => self.canvas.zoom_to = Some(canvas::ZoomTo::Selection),
            "canvas.zoom_100" => self.canvas.zoom_to = Some(canvas::ZoomTo::Level(1.0)),
            "canvas.zoom_in" => self.canvas.zoom_to = Some(canvas::ZoomTo::In),
            "canvas.zoom_out" => self.canvas.zoom_to = Some(canvas::ZoomTo::Out),
            "canvas.select_all" => self.select_all_nodes(),
            "canvas.toggle" => self.toggle_nodes(),
            "canvas.duplicate" => self.duplicate_nodes(),
            // the selected nodes framed together under a name (its title's typed on the canvas)
            "canvas.group" if !self.canvas.selected.is_empty() => self.run("group.add", json!({ "nodes": self.canvas.selected })),
            "canvas.delete" => self.delete_selection(),
            _ => self.run(id, json!({})),
        }
    }

    /// Puts a scene on the output with its launch settings (on its boundary, once built).
    fn launch(&mut self, scene: &str) {
        if let (Some(live), Some(scene)) = (&mut self.live, self.session.project.scene(scene)) {
            live.launch(scene);
        }
    }

    /// A scene left out of the autopilot is on the output: launched by hand, it stays until the
    /// autopilot is asked back (`stage.auto`) or another scene is launched.
    fn held_by_hand(&self) -> bool {
        let live = self.live.as_ref().and_then(|l| l.live_scene());
        live.and_then(|id| self.session.project.scene(id)).is_some_and(|s| s.manual && s.layer == Layer::Scene)
    }

    /// The autopilot's next scene (any but the one out) launched now, `launch`'s way or its own,
    /// whether the autopilot is on or not; its next turn counts from here.
    fn turn(&mut self, launch: Option<Launch>) {
        let Some(live) = &mut self.live else { return };
        let random = (live.beats() * 7919.0).fract().abs();
        let Some(id) = next_turn(&self.session.project.scenes, live.live_scene(), Some(random)) else { return };
        let Some(scene) = self.session.project.scene(&id) else { return };
        match launch {
            Some(launch) => live.launch_with(scene, launch),
            None => live.launch(scene),
        }
        self.pilot.next = None;
    }

    /// A dialog or menu has the keyboard.
    fn modal(&self) -> bool {
        self.palette.is_some() || self.pending.is_some() || self.recovery.is_some() || self.rename.is_some() || self.show_help || self.canvas.menu.is_some()
    }

    // ---- input --------------------------------------------------------------

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.text_edit_focused(); // a focused button or knob still lets shortcuts through
        if typing || self.modal() {
            self.space = None;
            return; // typing in a field or a dialog is open
        }
        // Esc leaves MIDI learn first (the canvas' Esc deselects)
        if (self.learn.on || self.learn.target.is_some()) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.learn.stop();
        }
        if self.assigning.is_some() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.assigning = None;
        }
        // the canvas' keys: bare ones (Tab, H, Delete…) where it has the keyboard, chords
        // (Cmd/Ctrl+A, Shift+1…) wherever it shows, as in Figma
        let canvas = self.canvas_keys(ctx);
        let shown = self.session.active().is_some();
        // the keymap; exact modifiers, so Ctrl+K never fires for Ctrl+Shift+K
        let bindings = self.settings.keymap.bindings();
        let mut fired = Vec::new();
        // Space is shared (docs/product.md §7.6): held with a drag it pans the canvas; a tap
        // fires its binding when it's let go
        const TAP: f64 = 0.35;
        let (now, pressing) = ctx.input(|i| (i.time, i.pointer.any_down()));
        let mut space = self.space.map(|(at, used)| (at, used || pressing));
        ctx.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key, physical_key, pressed, repeat: false, modifiers } => {
                    let key = keymap::chord_key(*key, *physical_key, *modifiers);
                    if key == egui::Key::Space && modifiers.is_none() {
                        if *pressed {
                            space = Some((now, false));
                            return false;
                        }
                        if !space.take().is_some_and(|(at, used)| !used && now - at < TAP) {
                            return false;
                        }
                    } else if !*pressed {
                        return true;
                    }
                    let ours = |id: &str, k: &egui::KeyboardShortcut| !id.starts_with("canvas.") || canvas || (shown && !k.modifiers.is_none());
                    let found = bindings.iter().find(|(id, k)| keymap::fires(key, *modifiers, k) && ours(id, k));
                    match found {
                        Some(&(id, _)) => {
                            fired.push(id);
                            false
                        }
                        None => true,
                    }
                }
                _ => true,
            })
        });
        self.space = space;
        for id in fired {
            self.command(id);
        }
    }

    // ---- regions (docs/product.md §7.1) -------------------------------------

    /// Scenes, as cards: LIVE on the one going out, a countdown on a launch; number keys
    /// launch them.
    fn scenes_rail(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        kit::region(ui);
        kit::bar(ui, &t, |ui| {
            kit::heading(ui, &t, icons::STACK, tr!("SCENES"));
            kit::right(ui, |ui| {
                if kit::icon_button(ui, &t, icons::CARET_LEFT, &self.tip(tr!("Hide scenes"), "app.toggle_scenes"), false).clicked() {
                    self.settings.panels.scenes = false;
                }
                self.autopilot_button(ui, &t);
                // cards with their pictures large, or slim rows: more of them in view
                let compact = self.settings.scenes_compact;
                let (icon, tip) = if compact { (icons::SQUARES_FOUR, tr!("Show the scenes as cards")) } else { (icons::LIST_BULLETS, tr!("Show the scenes as a compact list")) };
                if kit::icon_button(ui, &t, icon, tip, false).clicked() {
                    self.settings.scenes_compact = !compact;
                }
                // a new scene: empty, or from a template
                let plus = kit::icon_button(ui, &t, icons::PLUS, &self.tip(tr!("New scene"), "scene.add"), false);
                kit::menu(&t, &plus).width(300.0).show(|ui| {
                    if ui.button(format!("{}  {}", icons::PLUS, tr!("Empty scene"))).clicked() {
                        self.run("scene.add", json!({}));
                    }
                    ui.separator();
                    kit::menu_title(ui, &t, tr!("FROM A TEMPLATE"));
                    for template in &templates::TEMPLATES {
                        let r = ui.button(format!("{}  {}", icons::WAVE_SINE, i18n::t(template.name))).on_hover_text(i18n::t(template.description));
                        if r.clicked() {
                            if let Err(e) = templates::add(&mut self.session, template, Some("template")) {
                                self.say(Tone::Error, e);
                            }
                            self.session.close_merge();
                        }
                    }
                });
            });
        });
        let scenes = self.session.project.scenes.clone();
        let active = self.session.active_scene.clone();
        // none yet: a hand of ghost cards, what a scene is, and the button to make one
        if scenes.is_empty() {
            let area = ui.available_rect_before_wrap().shrink2(egui::vec2(t.pad * 1.5, 0.0));
            let spread = ui.rect_contains_pointer(ui.max_rect());
            ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.add_space((area.height() * 0.4 - 130.0).max(t.pad * 2.0));
                widgets::scene_deck(ui, &t, spread);
                ui.add_space(t.pad);
                ui.label(RichText::new(tr!("No scenes yet")).size(t.text_lg).color(t.fg_text));
                ui.label(RichText::new(tr!("A scene is one look of your show, launched by its number key.")).font(t.small()).color(t.fg_muted));
                ui.add_space(t.pad);
                if kit::button(ui, &t, Some(icons::PLUS), tr!("New scene"), false).clicked() {
                    self.run("scene.add", json!({}));
                }
            });
            return;
        }
        kit::scroll(ui, &t, "scenes", true, t.bg_panel, |ui| {
            ui.spacing_mut().item_spacing.y = if self.settings.scenes_compact { 2.0 } else { t.gap * 1.5 };
            let cards: Vec<egui::Rect> = scenes.iter().enumerate().map(|(i, scene)| self.scene_card(ui, &t, i, scene, active.as_ref() == Some(&scene.id), scenes.len())).collect();
            self.scene_drop(ui, &t, &scenes, &cards);
        });
    }

    /// A scene card dragged over the list: a line where it would land, the move on release.
    fn scene_drop(&mut self, ui: &egui::Ui, t: &Theme, scenes: &[Scene], cards: &[egui::Rect]) {
        let Some(drag) = egui::DragAndDrop::payload::<widgets::SceneDrag>(ui.ctx()) else { return };
        let Some(from) = scenes.iter().position(|s| s.id == drag.0) else { return };
        let Some(p) = ui.ctx().pointer_interact_pos().filter(|p| ui.clip_rect().contains(*p)) else { return };
        // before the first card whose middle is under the pointer
        let slot = cards.iter().filter(|r| r.center().y < p.y).count();
        let to = if slot > from { slot - 1 } else { slot };
        if to == from {
            return;
        }
        let gap = ui.spacing().item_spacing.y / 2.0;
        let y = if slot == 0 { cards[0].top() - gap } else { cards[slot - 1].bottom() + gap };
        ui.painter().hline(cards[0].x_range(), y.max(ui.clip_rect().top() + 1.0), egui::Stroke::new(2.0, t.accent));
        if ui.input(|i| i.pointer.any_released()) {
            egui::DragAndDrop::clear_payload(ui.ctx());
            self.run("scene.move", json!({ "scene": drag.0, "to": to }));
        }
    }

    /// One scene's card, dragged by its body to another place; where it is.
    fn scene_card(&mut self, ui: &mut egui::Ui, t: &Theme, i: usize, scene: &Scene, active: bool, count: usize) -> egui::Rect {
        // LIVE pulses with the beat; a launch counts down to its boundary
        let layer = (scene.layer != Layer::Scene).then(|| (scene.layer == Layer::Top, self.live.as_ref().is_none_or(|l| !l.off.contains(&scene.id))));
        let state = match &self.live {
            Some(l) => widgets::CardState { selected: active, live: (l.live_scene() == Some(scene.id.as_str())).then(|| l.clock()[1]), armed: l.armed(&scene.id), shot: l.shot(&scene.id), layer },
            None => widgets::CardState { selected: active, layer, ..Default::default() },
        };
        // the card itself drags (a drag-only widget over it would eat its clicks)
        let draw = if self.settings.scenes_compact { widgets::scene_row } else { widgets::scene_card };
        let resp = draw(ui, t, i, &scene.name, &scene.icon, widgets::hue_of(&scene.id), state).interact(egui::Sense::drag());
        if resp.drag_started() {
            egui::DragAndDrop::set_payload(ui.ctx(), widgets::SceneDrag(scene.id.clone()));
        }
        if resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        let rect = resp.rect;
        self.hint(&resp, tr!("Click to select · its number key launches it · drag to reorder · double-click to rename · right-click for more"));
        let target = Target::LaunchScene(i);
        if self.learnable(ui, &resp, &target) {
            return rect;
        }
        if resp.clicked() && !active {
            self.run("scene.select", json!({ "scene": scene.id }));
        }
        if resp.double_clicked() {
            self.rename = Some(shell::Renaming::new(&scene.id, &scene.name));
        }
        kit::context_menu(t, &resp).show(|ui| {
            if ui.button(format!("{}  {}", icons::PLAY, tr!("Launch"))).clicked() {
                self.launch(&scene.id);
            }
            if ui.button(format!("{}  {}", icons::PENCIL_SIMPLE, tr!("Rename…"))).clicked() {
                self.rename = Some(shell::Renaming::new(&scene.id, &scene.name));
            }
            if ui.button(format!("{}  {}", icons::COPY, tr!("Duplicate"))).clicked() {
                self.run("scene.duplicate", json!({ "scene": scene.id }));
            }
            ui.menu_button(format!("{}  {}", icons::ARROWS_DOWN_UP, tr!("Move")), |ui| {
                let last = count - 1;
                for (label, to, can) in [(tr!("To the top"), 0, i > 0), (tr!("Up"), i.saturating_sub(1), i > 0), (tr!("Down"), i + 1, i < last), (tr!("To the bottom"), last, i < last)] {
                    if ui.add_enabled(can, egui::Button::new(label)).clicked() {
                        self.run("scene.move", json!({ "scene": scene.id, "to": to }));
                    }
                }
            });
            if ui.button(format!("{}  {}", icons::FLOPPY_DISK, tr!("Export as a project…"))).on_hover_text(tr!("One file with this scene and its media, to share or to start a show from")).clicked() {
                self.export_dialog(Some(&scene.id));
            }
            // a layer: always over or under whichever scene goes out
            ui.menu_button(format!("{}  {}", icons::STACK, tr!("Layer")), |ui| {
                let roles = [(Layer::Scene, tr!("A scene, launched like the others")), (Layer::Top, tr!("Always on top")), (Layer::Bottom, tr!("Always underneath"))];
                for (layer, label) in roles {
                    if kit::menu_tick(ui, t, scene.layer == layer, label).clicked() {
                        self.run("scene.set_layer", json!({ "scene": scene.id, "layer": layer }));
                        ui.close();
                    }
                }
                if scene.layer != Layer::Scene {
                    ui.separator();
                    kit::menu_title(ui, t, tr!("BLEND"));
                    let blends = [(LayerBlend::Over, tr!("Over, through its transparency")), (LayerBlend::Add, tr!("Add")), (LayerBlend::Screen, tr!("Screen: lights on black")), (LayerBlend::Multiply, tr!("Multiply: darks on white"))];
                    for (blend, label) in blends {
                        if kit::menu_tick(ui, t, scene.blend == blend, label).clicked() {
                            self.run("scene.set_layer", json!({ "scene": scene.id, "blend": blend }));
                            ui.close();
                        }
                    }
                }
            });
            if scene.layer == Layer::Scene && kit::menu_tick(ui, t, !scene.manual, tr!("In the autopilot")).clicked() {
                self.run("scene.set_autopilot", json!({ "scene": scene.id, "on": scene.manual }));
            }
            ui.menu_button(format!("{}  {}", if scene.icon.is_empty() { "✨" } else { scene.icon.as_str() }, tr!("Icon")), |ui| {
                if let Some(icon) = widgets::emoji_grid(ui, &scene.icon) {
                    self.run("scene.set_icon", json!({ "scene": scene.id, "icon": icon }));
                    ui.close();
                }
            });
            // a whole scene gone: held, not clicked
            if kit::hold_button(ui, t, Some(icons::TRASH), tr!("Hold to delete"), tr!("Deleted"), 0.8) {
                self.run("scene.remove", json!({ "scene": scene.id }));
            }
            ui.separator();
            self.learn_items(ui, &target);
        });
        rect
    }

    /// The middle of the window, between the floating panels: the node canvas shows through
    /// (it's drawn under everything), or the layers' empty state. A floating toolbar, Figma
    /// UI3's, names the scene and switches the view.
    /// The split canvas' two parts (the nodes', the picture's), from the center as it was last
    /// frame; none unsplit.
    fn split_panes(&self) -> Option<(egui::Rect, egui::Rect)> {
        let view = self.center_rect?;
        let share = if self.settings.split_share > 0.0 { self.settings.split_share } else { 0.4 };
        match self.settings.split {
            Split::Off => None,
            Split::Right => {
                let x = (view.right() - view.width() * share).round();
                Some((egui::Rect::from_min_max(view.min, egui::pos2(x, view.bottom())), egui::Rect::from_min_max(egui::pos2(x, view.top()), view.max)))
            }
            Split::Bottom => {
                let y = (view.bottom() - view.height() * share).round();
                Some((egui::Rect::from_min_max(view.min, egui::pos2(view.right(), y)), egui::Rect::from_min_max(egui::pos2(view.left(), y), view.max)))
            }
        }
    }

    /// The split canvas' picture: the scene going out, whole, under a bar saying what it is;
    /// its inner edge drags to share the canvas differently.
    fn picture_pane(&mut self, ui: &mut egui::Ui, t: &Theme, pane: egui::Rect) {
        let painter = ui.painter_at(pane);
        painter.rect_filled(pane, 0, t.bg_base);
        let right = self.settings.split == Split::Right;
        let edge =
            if right { egui::Rect::from_min_max(pane.min - egui::vec2(3.0, 0.0), egui::pos2(pane.left() + 3.0, pane.bottom())) } else { egui::Rect::from_min_max(pane.min - egui::vec2(0.0, 3.0), egui::pos2(pane.right(), pane.top() + 3.0)) };
        let drag = ui.interact(edge, Id::unique("split-edge"), egui::Sense::drag());
        let hot = drag.hovered() || drag.dragged();
        if right {
            painter.vline(pane.left(), pane.y_range(), egui::Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { t.accent } else { t.line }));
        } else {
            painter.hline(pane.x_range(), pane.top(), egui::Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { t.accent } else { t.line }));
        }
        if hot {
            ui.ctx().set_cursor_icon(if right { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
        }
        if let (Some(p), Some(view)) = (drag.interact_pointer_pos().filter(|_| drag.dragged()), self.center_rect) {
            let share = if right { (view.right() - p.x) / view.width() } else { (view.bottom() - p.y) / view.height() };
            self.settings.split_share = share.clamp(0.15, 0.85);
        }
        let name = self.session.active().map(|s| s.name.clone()).unwrap_or_default();
        let on_air = self.live.as_ref().is_none_or(|l| l.live_scene() == self.session.active().map(|s| s.id.as_str()));
        let bar = egui::Rect::from_min_size(pane.min, egui::vec2(pane.width(), t.row_h));
        let (what, color) = if on_air { (tr!("LIVE"), t.accent) } else { (tr!("PREVIEW"), t.fg_muted) };
        let label = painter.text(bar.left_center() + egui::vec2(t.pad, 0.0), egui::Align2::LEFT_CENTER, what, Theme::mono(t.text_xs), color);
        kit::fit_text(&painter, label.right_center() + egui::vec2(t.gap, 0.0), egui::Align2::LEFT_CENTER, format!("· {name}"), t.small(), t.fg_text, bar.right() - label.right() - t.pad);
        let area = egui::Rect::from_min_max(egui::pos2(pane.left(), bar.bottom()), pane.max).shrink(t.pad);
        let c = self.session.project.canvas;
        let aspect = c.width.max(1) as f32 / c.height.max(1) as f32;
        let size = if area.width() / area.height().max(1.0) > aspect { egui::vec2(area.height() * aspect, area.height()) } else { egui::vec2(area.width(), area.width() / aspect) };
        let rect = egui::Rect::from_center_size(area.center(), size);
        match self.live.as_ref().and_then(live::Live::picture) {
            Some(id) => egui::Image::new((id, size)).corner_radius(t.radius_sm).paint_at(ui, rect),
            None => {
                painter.rect_filled(rect, t.radius_sm, t.bg_field);
                kit::fit_text(&painter, rect.center(), egui::Align2::CENTER_CENTER, tr!("Nothing goes out yet"), t.small(), t.fg_muted, rect.width());
            }
        }
    }

    fn center(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let whole = ui.max_rect();
        self.center_rect = Some(whole);
        // split: the picture's part, then the inspector docked at the nodes' edge
        let mut view = whole;
        if let Some((nodes, picture)) = self.split_panes() {
            self.picture_pane(ui, &t, picture);
            view = nodes;
            if self.settings.panels.inspector && !self.settings.inspector_floating {
                let column = egui::Rect::from_min_max(egui::pos2(view.right() - 268.0, view.top()), view.max);
                ui.painter().rect_filled(column, 0, t.bg_panel);
                ui.painter().vline(column.left(), column.y_range(), egui::Stroke::new(1.0, t.line));
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(column.shrink2(egui::vec2(0.0, 0.0))));
                inner.set_clip_rect(column);
                kit::docked(&mut inner, |ui| self.inspector(ui));
                view.max.x = column.left();
            }
        }
        self.canvas.view = Some(view);
        if self.settings.hud {
            self.hud(ui.ctx(), &t, view);
        }
        let Some(name) = self.session.active().map(|s| s.name.clone()) else {
            kit::empty_state(ui, &t, icons::STACK, tr!("No scene selected"), tr!("Add a scene on the left to start."));
            return;
        };
        self.canvas.covered = None;
        if !self.settings.hide_preview && self.settings.split == Split::Off {
            self.preview_card(ui.ctx(), &t, view);
        }
        // centered, or at the left when the nodes' part is narrow (the zoom sits at the right)
        let (at, align) = if view.width() < 820.0 { (view.left_bottom() + egui::vec2(t.pad, -t.pad), egui::Align2::LEFT_BOTTOM) } else { (view.center_bottom() - egui::vec2(0.0, t.pad), egui::Align2::CENTER_BOTTOM) };
        kit::pill(ui.ctx(), &t, "toolbar", at, align, |ui| {
            ui.add_space(t.pad - t.gap);
            ui.label(RichText::new(&name).color(t.fg_text));
            kit::vsep(ui, &t);
            if kit::icon_button(ui, &t, icons::PLUS, &self.tip(tr!("Add a node"), "canvas.add"), false).clicked() {
                self.canvas.open_menu = true;
            }
            let split = self.settings.split != Split::Off;
            if kit::icon_button(ui, &t, icons::SQUARE_SPLIT_HORIZONTAL, &self.tip(tr!("Split the canvas: the picture beside the nodes, under them, or off"), "app.split"), split).clicked() {
                self.command("app.split");
            }
            if kit::icon_button(ui, &t, icons::CROSSHAIR, &self.tip(tr!("Zoom to fit"), "canvas.home"), false).clicked() {
                self.canvas.zoom_to = Some(canvas::ZoomTo::Fit);
            }
            if kit::icon_button(ui, &t, icons::TREE_STRUCTURE, tr!("Arrange the nodes"), false).clicked() {
                self.canvas.arrange = true;
            }
            let tie = self.canvas.tie_tool;
            if kit::icon_button(ui, &t, icons::GIT_MERGE, tr!("Tie cables: drag a line across them (or Alt+drag any time)"), tie).clicked() {
                self.canvas.tie_tool = !tie;
            }
            let hidden = self.canvas.hidden;
            if kit::icon_button(ui, &t, icons::EYE, if hidden { tr!("Show the nodes") } else { tr!("Hide the nodes: the picture alone") }, hidden).clicked() {
                self.canvas.hidden = !hidden;
            }
        });
    }

    /// The autopilot's button: lit while it plays, its settings in a popover.
    fn autopilot_button(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let a = &mut self.settings.autopilot;
        let tip = match (a.on, a.timed) {
            (false, _) => tr!("Autopilot: off").to_string(),
            (true, false) => tr!("Autopilot: a scene every {} bars", a.bars),
            (true, true) => tr!("Autopilot: a scene every {}", clock(a.seconds)),
        };
        let r = kit::icon_button(ui, t, icons::AIRPLANE_TILT, &tip, a.on);
        let popup = egui::Popup::from_toggle_button_response(&r).frame(kit::popover_frame(ui.ctx(), t)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(260.0);
        popup.show(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.gap * 2.0, t.gap * 2.0);
            kit::setting(ui, t, tr!("Autopilot"), tr!("Launches another scene by itself, each its own way"), |ui| kit::switch(ui, t, &mut a.on, tr!("Autopilot")));
            // on the music's bars, or on the clock
            kit::label(ui, t, tr!("Every"));
            if let Some(i) = kit::segmented(ui, t, usize::from(a.timed), &[tr!("In bars"), tr!("In seconds")]) {
                a.timed = i == 1;
            }
            if a.timed {
                let presets = [30, 60, 180, 300];
                let at = presets.iter().position(|s| *s == a.seconds).unwrap_or(usize::MAX);
                if let Some(i) = kit::segmented(ui, t, at, &presets.map(clock).each_ref().map(String::as_str)) {
                    a.seconds = presets[i];
                }
                kit::scrub(ui, t, tr!("SECONDS"), &mut a.seconds, 5..=3600, ui.available_width());
            } else {
                let bars = [4, 8, 16, 32];
                let at = bars.iter().position(|b| *b == a.bars).unwrap_or(1);
                if let Some(i) = kit::segmented(ui, t, at, &[tr!("4 bars"), tr!("8 bars"), tr!("16 bars"), tr!("32 bars")]) {
                    a.bars = bars[i];
                }
            }
            kit::label(ui, t, tr!("Order"));
            if let Some(i) = kit::segmented(ui, t, usize::from(a.random), &[tr!("In order"), tr!("At random")]) {
                a.random = i == 1;
            }
            kit::setting(ui, t, tr!("On drops"), tr!("The music back at full after a quieter while launches the next one too"), |ui| kit::switch(ui, t, &mut a.drops, tr!("On drops")));
        });
    }

    /// Parameters of whatever is selected; with nothing selected, the canvas.
    fn inspector(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        kit::region(ui);
        kit::bar(ui, &t, |ui| {
            kit::heading(ui, &t, icons::SLIDERS, tr!("INSPECTOR"));
            kit::right(ui, |ui| {
                let (icon, close) = if self.settings.inspector_floating { (icons::X, tr!("Hide inspector")) } else { (icons::CARET_RIGHT, tr!("Hide inspector")) };
                if kit::icon_button(ui, &t, icon, &self.tip(close, "app.toggle_inspector"), false).clicked() {
                    self.settings.panels.inspector = false;
                }
                let (icon, tip) = if self.settings.inspector_floating { (icons::SIDEBAR_SIMPLE, tr!("Dock the inspector")) } else { (icons::ARROW_SQUARE_OUT, tr!("Inspector in a window of its own")) };
                if kit::icon_button(ui, &t, icon, &self.tip(tip, "app.float_inspector"), false).clicked() {
                    self.command("app.float_inspector");
                }
            });
        });
        let selected = self.canvas.selected.clone();
        // one node: its name and what's done to it stay on top, its settings scroll under them;
        // none, the scene's
        match selected.as_slice() {
            [one] => self.node_header(ui, &t, one),
            [] => self.scene_header(ui, &t),
            _ => {}
        }
        let (key, now) = (Id::unique("inspected"), ui.input(|i| i.time));
        let since = ui.data(|d| d.get_temp::<(Vec<String>, f64)>(key)).filter(|(was, _)| *was == selected).map_or(now, |(_, at)| at);
        ui.data_mut(|d| d.insert_temp(key, (selected.clone(), since)));
        let shown = if t.motion > 0.0 { kit::ease(((now - since) / 0.2) as f32) } else { 1.0 };
        if shown < 1.0 {
            ui.ctx().request_repaint();
        }
        ui.multiply_opacity(shown);
        kit::scroll(ui, &t, ("inspector", selected.first()), true, t.bg_panel, |ui| {
            if let [one] = selected.as_slice() {
                return self.node_inspector(ui, &t, one);
            }
            if selected.len() > 1 {
                let duplicate = self.settings.keymap.label("canvas.duplicate");
                kit::section(ui, &t, icons::SELECTION_PLUS, tr!("SELECTION"), true, |ui| {
                    ui.label(RichText::new(tr!("{} nodes selected", selected.len())).font(t.body()).color(t.fg_text));
                    kit::hint(ui, &t, &tr!("Drag one to move them all. Delete removes them, {} duplicates them.", duplicate));
                    ui.add_space(t.gap);
                    // framed together under a name, to keep a big graph in order
                    if kit::button(ui, &t, Some(icons::SQUARES_FOUR), &self.tip(tr!("Group"), "canvas.group"), false).clicked() {
                        self.command("canvas.group");
                    }
                });
                return;
            }
            let first = !self.launch_settings(ui, &t);
            kit::section(ui, &t, icons::FRAME_CORNERS, tr!("CANVAS"), first, |ui| {
                let c = self.session.project.canvas;
                // the usual sizes, or any: W, H and the frame rate scrub like Figma's fields
                let preset = RESOLUTIONS.iter().find(|r| (r.1, r.2) == (c.width, c.height));
                let text = preset.map_or_else(|| tr!("Custom · {}×{}", c.width, c.height), |r| format!("{} · {}×{}", i18n::t(r.0), r.1, r.2));
                let r = kit::select(ui, &t, None, &text);
                let mut pick = None;
                kit::menu(&t, &r).show(|ui| {
                    let labels: Vec<String> = RESOLUTIONS.iter().map(|(name, w, h)| format!("{} · {w}×{h}", i18n::t(name))).collect();
                    let items: Vec<(&str, &str)> = labels.iter().map(|l| ("", l.as_str())).collect();
                    pick = kit::choose(ui, &t, &items, RESOLUTIONS.iter().position(|r| preset.is_some_and(|p| p.0 == r.0))).map(|i| (RESOLUTIONS[i].1, RESOLUTIONS[i].2));
                });
                if let Some((w, h)) = pick {
                    self.run("project.set_canvas", json!({ "width": w, "height": h }));
                }
                ui.add_space(t.gap);
                let width = (ui.available_width() - 2.0 * t.gap) / 3.0;
                ui.horizontal(|ui| {
                    for ((label, mut value), (key, info)) in [("W", c.width), ("H", c.height), ("FPS", c.fps)].into_iter().zip(["width", "height", "fps"].into_iter().zip(&CANVAS)) {
                        let r = kit::scrub(ui, &t, label, &mut value, info.min as u32..=info.max as u32, width);
                        if r.changed() {
                            self.run_merge("project.set_canvas", json!({ key: value }), key);
                        }
                        if r.drag_stopped() || r.lost_focus() {
                            self.session.close_merge();
                        }
                    }
                });
                kit::hint(ui, &t, tr!("The size and frame rate every output renders at."));
            });
        });
    }

    /// The selected scene's launch settings (docs/scenes.md §3): how it replaces the live
    /// scene, and on which boundary. False with no scene.
    fn launch_settings(&mut self, ui: &mut egui::Ui, t: &Theme) -> bool {
        let Some(scene) = self.session.active() else { return false };
        let (id, launch) = (scene.id.clone(), scene.launch);
        let (mut set, mut close) = (None, false);
        kit::section(ui, t, icons::PLAY_CIRCLE, tr!("LAUNCH"), true, |ui| {
            let names: Vec<&str> = Transition::ALL.iter().map(|x| i18n::t(x.label())).collect();
            let at = Transition::ALL.iter().position(|x| *x == launch.transition).unwrap_or(0);
            let r = kit::select_named(ui, t, tr!("Transition"), names[at]);
            kit::menu(t, &r).show(|ui| {
                let items: Vec<(&str, &str)> = names.iter().map(|n| ("", *n)).collect();
                if let Some(i) = kit::choose(ui, t, &items, Some(at)) {
                    set = Some(json!({ "transition": Transition::ALL[i] }));
                }
            });
            kit::label(ui, t, tr!("Starts on"));
            let at = Quantize::ALL.iter().position(|x| *x == launch.quantize).unwrap_or(0);
            if let Some(i) = kit::segmented(ui, t, at, &[tr!("Now"), tr!("Beat"), tr!("Bar"), tr!("4 bars")]) {
                set = Some(json!({ "quantize": Quantize::ALL[i] }));
            }
            if launch.transition != Transition::Cut {
                ui.add_space(t.gap);
                // how long the transition takes, in beats: a number to scrub, as the canvas' size
                let mut beats = launch.duration.round().max(1.0) as u32;
                let r = kit::scrub(ui, t, tr!("BEATS"), &mut beats, LAUNCH_BEATS.min as u32..=LAUNCH_BEATS.max as u32, ui.available_width());
                if r.changed() {
                    set = Some(json!({ "duration": beats as f32 }));
                }
                close = r.drag_stopped() || r.lost_focus();
            }
            ui.add_space(t.gap);
            kit::hint(ui, t, tr!("Launch it with its number key, or with Space while it's selected."));
        });
        if let Some(mut props) = set {
            props["scene"] = json!(id);
            let merge = props.get("duration").is_some();
            if merge {
                self.run_merge("scene.set_launch", props, &format!("launch {id}"));
            } else {
                self.run("scene.set_launch", props);
            }
        }
        if close {
            self.session.close_merge();
        }
        true
    }

    /// The selected scene while no node is: its name (a click renames it), how many nodes it
    /// has, and Launch, or LIVE while it's on air.
    fn scene_header(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let Some(scene) = self.session.active() else { return };
        let (id, name, count) = (scene.id.clone(), scene.name.clone(), scene.nodes.len());
        let on_air = self.live.as_ref().is_some_and(|l| l.live_scene() == Some(id.as_str()));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.bar_h + 20.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, t.bg_panel);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, t.line));
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(t.pad, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
        row.spacing_mut().item_spacing = egui::vec2(t.gap, 0.0);
        let (mut launch, mut rename) = (false, false);
        if on_air {
            row.label(RichText::new(tr!("LIVE")).font(Theme::mono(t.text_xs)).extra_letter_spacing(1.0).color(t.accent));
        } else {
            let tip = self.tip(tr!("Launch"), "stage.launch");
            launch = kit::button(&mut row, t, Some(icons::PLAY), tr!("Launch"), false).on_hover_text(tip).clicked();
        }
        row.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.gap * 2.0, 2.0);
            let (spot, _) = ui.allocate_exact_size(egui::vec2(t.control_h, t.control_h), egui::Sense::hover());
            ui.painter().rect_filled(spot, t.radius_sm, Theme::mix(t.bg_control, t.accent, if on_air { 0.25 } else { 0.1 }));
            kit::icon_at(ui.painter(), spot.center(), icons::STACK, 15.0, if on_air { t.accent } else { t.fg_text });
            let room = ui.available_rect_before_wrap();
            let (left, mid, width) = (room.left(), room.center().y, room.width());
            let (named, _) = kit::fit_text(ui.painter(), egui::pos2(left, mid - 8.0), egui::Align2::LEFT_CENTER, &name, t.body(), t.fg_text, width);
            let what = if count == 1 { tr!("Scene · 1 node").to_string() } else { tr!("Scene · {} nodes", count) };
            kit::fit_text(ui.painter(), egui::pos2(left, mid + 9.0), egui::Align2::LEFT_CENTER, what, t.small(), t.fg_muted, width);
            rename = ui.interact(named, egui::Id::unique(("scene name", &id)), egui::Sense::click()).on_hover_text(tr!("Rename…")).clicked();
        });
        if launch {
            self.launch(&id);
        }
        if rename {
            self.rename = Some(shell::Renaming::new(id, name));
        }
    }

    /// One node: its name and kind, then each port and what it's wired to.
    fn node_inspector(&mut self, ui: &mut egui::Ui, t: &Theme, id: &str) {
        let Some(scene) = self.session.active() else { return };
        let Some(node) = scene.node(id) else { return };
        let kind = node_kind(&node.kind);
        let name_of = |id: &str| scene.node(id).map_or_else(|| id.to_string(), |n| n.name.clone());
        let port_name = |id: &str, port: u8, output: bool| {
            let k = scene.node(id).and_then(|n| node_kind(&n.kind));
            let names = k.map_or(&[][..], |k| if output { k.outputs } else { k.inputs });
            names.get(usize::from(port)).map_or_else(|| port.to_string(), |n| n.to_string())
        };
        let inputs: Vec<(String, Vec<String>)> = kind
            .map_or(&[][..], |k| k.inputs)
            .iter()
            .take(velo_core::inputs_shown(scene, node))
            .enumerate()
            .map(|(i, p)| (p.to_string(), scene.wire_into(id, i as u8).map(|e| format!("{} · {}", name_of(&e.from.0), port_name(&e.from.0, e.from.1, true))).into_iter().collect()))
            .collect();
        let outputs: Vec<(String, Vec<String>)> = kind
            .map_or(&[][..], |k| k.outputs)
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let wires = scene.edges.iter().filter(|e| e.from.0 == id && usize::from(e.from.1) == i).map(|e| format!("{} · {}", name_of(&e.to.0), port_name(&e.to.0, e.to.1, false)));
                // and what it moves (a signal's dashed wires)
                let moves = scene.nodes.iter().flat_map(|n| n.mappings.iter().filter(move |m| velo_core::signal_output(&m.source) == Some((id, i as u8))).map(move |m| format!("{} · {}", n.name, param_name(n, &m.param))));
                (p.to_string(), wires.chain(moves).collect())
            })
            .collect();
        let (node_id, kind_id) = (node.id.clone(), node.kind.clone());
        // what comes in and goes out, one section drawn as a patch bay: the node's edge down the
        // left, a jack a port, a cable off to what it's wired to. Past four ports a side, the
        // wired ones and a count of the rest
        kit::section(ui, t, icons::PLUGS_CONNECTED, tr!("WIRES"), true, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let sides = [(&inputs, true), (&outputs, false)];
            let shown: Vec<(&String, &Vec<String>, bool)> = sides.iter().flat_map(|(ports, incoming)| ports.iter().filter(|(_, to)| ports.len() <= 4 || !to.is_empty()).map(|(port, to)| (port, to, *incoming))).collect();
            for (i, (port, to, incoming)) in shown.iter().enumerate() {
                widgets::wire_row(ui, t, port, *incoming, to, (i > 0, i + 1 < shown.len()));
            }
            let free: usize = sides.iter().filter(|(ports, _)| ports.len() > 4).map(|(ports, _)| ports.iter().filter(|(_, to)| to.is_empty()).count()).sum();
            if free > 0 {
                ui.add_space(t.gap);
                kit::hint(ui, t, &tr!("{} more, not wired", free));
            }
        });
        if velo_core::is_shader(&kind_id) {
            self.preset_section(ui, t, &node_id);
            self.code_section(ui, t, &node_id);
        }
        if velo_core::shows_media(&kind_id) {
            self.media_section(ui, t, &node_id, false);
        }
        if kind_id == velo_core::MEDIA_LIST {
            self.media_list_section(ui, t, &node_id);
        }
        if kind_id == velo_core::PARTICLES {
            self.sprite_section(ui, t, &node_id);
        }
        if kind_id == velo_core::MESH {
            self.mesh_section(ui, t, &node_id);
        }
        if kind_id == velo_core::LIVE {
            self.live_section(ui, t, &node_id);
        }
        if kind_id == "velo.grid" {
            self.grid_section(ui, t, &node_id, false);
        } else {
            self.node_params(ui, t, &node_id, false);
        }
    }

    /// The selected node, over its scrolling inspector: its icon, its name (a click renames
    /// it) and kind, and what's done to the node itself: on or off, duplicate, delete.
    fn node_header(&mut self, ui: &mut egui::Ui, t: &Theme, id: &str) {
        let Some(scene) = self.session.active() else { return };
        let Some(node) = scene.node(id) else { return };
        let kind = node_kind(&node.kind);
        let (name, scene_id, icon, off) = (node.name.clone(), scene.id.clone(), node.icon.clone(), node.off);
        // what kind of node it is; renamed, its kind too
        let what = kind.map_or_else(
            || node.kind.clone(),
            |k| {
                let role = i18n::t(role_of(k));
                if node.name == k.label || node.name == i18n::t(k.label) { role.to_string() } else { format!("{} · {role}", i18n::t(k.label)) }
            },
        );
        let dot = kind.map_or(t.fg_muted, |k| t.category(k.category));
        let output = kind.is_some_and(|k| k.category == Category::Output);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.bar_h + 20.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 0.0, t.bg_panel);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, t.line));
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(t.pad, 0.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
        row.spacing_mut().item_spacing = egui::vec2(t.gap, 0.0);
        let (mut act, mut new_icon) = (None, None);
        if kit::icon_button(&mut row, t, icons::TRASH, &self.tip(tr!("Delete node"), "canvas.delete"), false).clicked() {
            act = Some("delete");
        }
        if kit::icon_button(&mut row, t, icons::COPY, &self.tip(tr!("Duplicate"), "canvas.duplicate"), false).clicked() {
            act = Some("duplicate");
        }
        if !output && kit::icon_button(&mut row, t, icons::POWER, &self.tip(if off { tr!("Turn on") } else { tr!("Turn off") }, "canvas.toggle"), !off).clicked() {
            act = Some("toggle");
        }
        row.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.gap * 2.0, 2.0);
            new_icon = widgets::icon_picker(ui, t, &icon, dot);
            // the name over the kind, centered on the icon; a click on the name renames it
            let room = ui.available_rect_before_wrap();
            let (left, mid, width) = (room.left(), room.center().y, room.width());
            let (named, _) = kit::fit_text(ui.painter(), egui::pos2(left, mid - 8.0), egui::Align2::LEFT_CENTER, &name, t.body(), t.fg_text, width);
            kit::fit_text(ui.painter(), egui::pos2(left, mid + 9.0), egui::Align2::LEFT_CENTER, what, t.small(), t.fg_muted, width);
            if ui.interact(named, egui::Id::unique(("node name", id)), egui::Sense::click()).on_hover_text(tr!("Rename… (or double-click the node)")).clicked() {
                act = Some("rename");
            }
        });
        if let Some(icon) = new_icon {
            self.run("node.set_icon", json!({ "node": id, "icon": icon }));
        }
        match act {
            Some("rename") => self.rename = Some(shell::Renaming::node(&scene_id, id, &name)),
            Some("toggle") => self.run("node.toggle", json!({ "nodes": [id] })),
            Some("duplicate") => {
                self.canvas.selected = vec![id.to_string()];
                self.duplicate_nodes();
            }
            Some("delete") => {
                self.canvas.selected = vec![id.to_string()];
                self.delete_selection();
            }
            _ => {}
        }
    }

    /// The deck under everything, the window's width (docs/scenes.md §2): what you play, in view
    /// whatever the inspector shows. The selected scene's variations (click recalls, Shift+click
    /// morphs over a bar, + stores where the macros are, the dice throws them somewhere new) and
    /// its eight macros (a controller drives them, learn binds one), then every source moving
    /// live, each dragged onto a slider to map it.
    fn deck(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let scene = self.session.active().map(|s| (s.id.clone(), s.macros.clone(), s.variations.clone()));
        // another scene's: it waited for a value there
        if self.assigning.as_ref().is_some_and(|(at, _)| scene.as_ref().is_none_or(|s| *at != s.0)) {
            self.assigning = None;
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(t.pad as i8, t.pad as i8)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = t.pad;
                // the sources as wide as they need, up to half the deck; the macros the rest
                let count = SOURCES.len() + self.session.active().map_or(0, |s| s.nodes.iter().filter(|n| n.kind == velo_core::LFO).count());
                let sources_w = (count as f32 * (widgets::METER_W + t.gap) + 2.0 * t.pad).min(ui.available_width() * 0.5);
                if let Some((id, macros, variations)) = &scene {
                    ui.vertical(|ui| {
                        ui.set_width(VARIATIONS_W);
                        self.variations_block(ui, &t, id, macros, variations);
                    });
                    kit::vsep(ui, &t);
                    let macros_w = ui.available_width() - sources_w - 2.0 * t.pad - 1.0;
                    ui.vertical(|ui| {
                        ui.set_width(macros_w);
                        self.macros_block(ui, &t, id, macros);
                    });
                    kit::vsep(ui, &t);
                }
                ui.vertical(|ui| self.sources_block(ui, &t));
            });
        });
    }

    /// A block's title in the deck, with what it's for in its tooltip and its own buttons on
    /// the right.
    fn deck_head<R>(ui: &mut egui::Ui, t: &Theme, icon: &str, title: &str, about: &str, right: impl FnOnce(&mut egui::Ui) -> R) -> R {
        ui.horizontal(|ui| {
            ui.set_height(DECK_HEAD);
            kit::heading(ui, t, icon, title).on_hover_text(about);
            kit::right(ui, right)
        })
        .inner
    }

    /// The variations: a pad for each, + to store one, the dice for somewhere new.
    fn variations_block(&mut self, ui: &mut egui::Ui, t: &Theme, id: &str, macros: &[velo_core::Macro], variations: &[velo_core::Variation]) {
        let random = Self::deck_head(ui, t, icons::GRID_FOUR, tr!("VARIATIONS"), tr!("Where the macros are, kept: click one to go back there, Shift+click to morph there over a bar"), |ui| {
            kit::icon_button(ui, t, icons::SHUFFLE, tr!("Every macro that drives something to a random position: click again for another"), false).clicked()
        });
        if random {
            self.run("macro.randomize", json!({ "scene": id }));
        }
        // a key each, four a row: the next one + to store, the rest where they'll be
        ui.horizontal_wrapped(|ui| {
            // room for the controllers' badges on the keys' corners
            ui.spacing_mut().item_spacing = egui::vec2(t.gap, t.pad);
            for (k, v) in variations.iter().enumerate() {
                self.variation_button(ui, t, id, k, v, macros);
            }
            let store = tr!("Store where the macros are now as a variation");
            if variations.len() < VARIATIONS && widgets::deck_key(ui, t, variation_key(t), icons::PLUS, store, false).on_hover_text(store).clicked() {
                self.run("variation.store", json!({ "scene": id }));
            }
            for k in variations.len() + 1..VARIATIONS {
                widgets::key_socket(ui, t, variation_key(t), &midi::variation_letter(k).to_string());
            }
        });
    }

    /// The scene's eight macros: knobs, an empty one a slot to give a value to.
    fn macros_block(&mut self, ui: &mut egui::Ui, t: &Theme, id: &str, macros: &[velo_core::Macro]) {
        // what each macro drives, for its tooltip
        let drives: Vec<String> = macros.iter().map(|m| m.targets.iter().map(|x| self.target_name(&Target::Param { node: x.node.clone(), param: x.param.clone() })).collect::<Vec<_>>().join(", ")).collect();
        if let Some((_, i)) = self.assigning {
            self.info_next = tr!("Touch any value in the inspector and {} drives it · Esc cancels", midi::macro_name(&macros[i], i));
        }
        Self::deck_head(ui, t, icons::GAUGE, tr!("MACROS"), tr!("This scene's eight knobs: click an empty one, then touch any value for it to drive; right-click one to rename it or learn MIDI"), |_| ());
        // spread over the block, a knob a slot; scrolling once they don't fit
        let slot = (ui.available_width() / macros.len().max(1) as f32).max(MACRO_SLOT);
        kit::hscroll(ui, "macros", |ui| {
            // the slots' tops in line
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (i, m) in macros.iter().enumerate() {
                    ui.allocate_ui_with_layout(egui::vec2(slot, MACROS_H + t.text_xs + 4.0), egui::Layout::top_down(egui::Align::Center), |ui| self.macro_knob(ui, t, id, i, m, &drives[i]));
                }
            });
        });
    }

    /// Every source moving live: the audio's levels, hits and melody, the beat clock, the
    /// scene's LFOs; each dragged onto a slider maps it.
    fn sources_block(&mut self, ui: &mut egui::Ui, t: &Theme) {
        let values = self.live.as_ref().map(live::Live::sources);
        let beats = self.live.as_ref().map_or(0.0, live::Live::beats);
        // the scene's LFOs: sources too, by their node's id
        let lfos: Vec<(String, String, f32)> = self
            .session
            .active()
            .map(|s| s.nodes.iter().filter(|n| n.kind == velo_core::LFO).map(|n| (n.id.clone(), n.name.clone(), velo_engine::signal::Lfo::from_params(|p| n.params.get(p).map(|v| v[0])).at(beats))).collect())
            .unwrap_or_default();
        Self::deck_head(ui, t, icons::WAVEFORM, tr!("SOURCES"), tr!("What moves with the music: drag one onto any slider and the music moves it"), |_| ());
        let Some(values) = values else {
            ui.label(RichText::new(tr!("The audio's levels, the beat and LFOs show here once the engine runs.")).small().color(t.fg_muted));
            return;
        };
        let height = MACROS_H + t.text_xs + 4.0;
        kit::hscroll(ui, "sources", |ui| {
            ui.spacing_mut().item_spacing.x = t.gap;
            // the audio's levels and melody, its hits, then the beat clock: meters, pads, lamps
            for i in [0, 1, 2, 3, 9, 4, 5, 6, 7, 8] {
                let id = SOURCES[i];
                if id == "kick" || id == "beat" {
                    ui.add_space(t.pad);
                }
                let kind = match id {
                    "kick" => widgets::Meter::Hit(widgets::TE_ORANGE),
                    "snare" => widgets::Meter::Hit(widgets::TE_BLUE),
                    "hat" => widgets::Meter::Hit(widgets::TE_GREEN),
                    "beat" => widgets::Meter::Beat,
                    "bar_phase" => widgets::Meter::Phase,
                    _ => widgets::Meter::Level,
                };
                widgets::source_meter(ui, t, id, source_label(id), values[i], kind, height).on_hover_text(tr!("{}: drag it onto any slider and the music moves it", source_label(id)));
            }
            if !lfos.is_empty() {
                ui.add_space(t.pad);
            }
            for (id, name, value) in &lfos {
                widgets::source_meter(ui, t, id, name, *value, widgets::Meter::Level, height).on_hover_text(tr!("{}: drag it onto any slider and the music moves it", name));
            }
        });
    }

    fn macro_knob(&mut self, ui: &mut egui::Ui, t: &Theme, scene: &str, i: usize, m: &velo_core::Macro, drives: &str) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let name = midi::macro_name(m, i);
        let target = Target::Macro(i as u8);
        // an empty one is a slot, not a knob turning nothing: click it, then touch a value
        let empty = m.targets.is_empty() && !self.learn.on;
        let waiting = self.assigning.as_ref().is_some_and(|a| a.0 == scene && a.1 == i);
        let learning = self.learn.target.as_ref() == Some(&target);
        // its cap's color, the OP-1's four in turn
        let cap = widgets::cap_color(t, i);
        widgets::numbered(ui, t, i + 1, &m.name, if empty && !waiting { t.fg_muted } else { cap });
        let (r, v) = if empty { (widgets::macro_slot(ui, t, waiting, cap), None) } else { widgets::deck_knob(ui, t, &MACRO, &name, m.value, cap, learning) };
        if empty && r.clicked() {
            self.assigning = (!waiting).then(|| (scene.to_string(), i));
        }
        // under it, the controller that turns it, else what it drives
        let (below, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.text_xs + 4.0), egui::Sense::hover());
        if let Some(label) = self.binding_label(&target) {
            ui.painter().text(below.center(), egui::Align2::CENTER_CENTER, label, Theme::mono(t.text_xs), t.accent);
        } else if !drives.is_empty() {
            let first = drives.split(", ").next().unwrap_or_default();
            let more = m.targets.len().saturating_sub(1);
            let short = if more > 0 { format!("{first} +{more}") } else { first.to_string() };
            kit::fit_text(ui.painter(), below.center(), egui::Align2::CENTER_CENTER, short.to_uppercase(), Theme::mono(t.text_xs - 1.0), t.fg_muted, below.width() - t.gap);
        }
        if self.learn.on || learning {
            widgets::learn_ring(ui, t, r.rect, learning);
        }
        if self.learn.on {
            if r.clicked() {
                self.learn.wait(target.clone());
            }
        } else if let Some(v) = v {
            self.run_merge("macro.set", json!({ "scene": scene, "index": i, "value": v }), &format!("macro {scene} {i}"));
        }
        if r.drag_stopped() || r.lost_focus() {
            self.session.close_merge();
        }
        let tip = if drives.is_empty() { tr!("{} drives nothing yet: click it, then touch any value in the inspector", name) } else { tr!("{} drives {} · right-click to rename or learn MIDI", name, drives) };
        self.hint(&r, tip);
        kit::context_menu(t, &r).show(|ui| {
            if ui.add_enabled(m.value != MACRO.default, egui::Button::new(format!("{}  {}", icons::ARROW_U_UP_LEFT, tr!("Reset")))).clicked() {
                self.run("macro.set", json!({ "scene": scene, "index": i, "value": MACRO.default }));
                ui.close();
            }
            if ui.button(format!("{}  {}", icons::PENCIL_SIMPLE, tr!("Rename…"))).clicked() {
                self.rename = Some(shell::Renaming::macro_(scene, i, &m.name));
                ui.close();
            }
            ui.separator();
            self.learn_items(ui, &target);
        });
    }

    /// A variation's button: its letter, lit while the macros are where it has them.
    fn variation_button(&mut self, ui: &mut egui::Ui, t: &Theme, scene: &str, k: usize, v: &velo_core::Variation, macros: &[velo_core::Macro]) {
        let on = v.macros.iter().zip(macros).all(|(a, m)| (a - m.value).abs() < 1e-4);
        let letter = midi::variation_letter(k).to_string();
        let r = widgets::deck_key(ui, t, variation_key(t), &letter, &letter, on);
        let target = Target::Variation(k);
        let what = if v.name.is_empty() { letter.clone() } else { format!("{letter} · {}", v.name) };
        self.hint(&r, tr!("Variation {}: click recalls it · Shift+click morphs there over a bar · right-click for more", what));
        if self.learnable(ui, &r, &target) {
            return;
        }
        let recall = |morph: bool| {
            let mut props = json!({ "scene": scene, "index": k });
            if morph {
                props["morph_beats"] = json!(MORPH_BEATS);
            }
            props
        };
        if r.clicked() {
            let shift = ui.input(|i| i.modifiers.shift);
            self.run("variation.recall", recall(shift));
        }
        kit::context_menu(t, &r).show(|ui| {
            if ui.button(format!("{}  {}", icons::PLAY, tr!("Recall"))).clicked() {
                self.run("variation.recall", recall(false));
                ui.close();
            }
            if ui.button(format!("{}  {}", icons::WAVE_SINE, tr!("Morph there over a bar"))).clicked() {
                self.run("variation.recall", recall(true));
                ui.close();
            }
            ui.separator();
            self.learn_items(ui, &target);
            ui.separator();
            if ui.button(format!("{}  {}", icons::TRASH, tr!("Remove"))).clicked() {
                self.run("variation.remove", json!({ "scene": scene, "index": k }));
                ui.close();
            }
        });
    }

    /// The engine follows the active scene; the preview comes back.
    /// A lost GPU or a render thread that stopped drawing: the event loop starts the engine
    /// again; a new one is taken up as at start, the scene that was out back out at once.
    fn watch_engine(&mut self, now: f64) {
        let unplugged = self.live.as_ref().and_then(|l| l.shared.borrow_mut().unplugged.take());
        if let Some(name) = unplugged {
            self.say(Tone::Error, tr!("{} was unplugged: the output closed", name));
        }
        let Some(live) = &self.live else { return };
        // VELO_DEMO_LOSE=seconds: the GPU device destroyed that long in, as a driver reset does
        // (tests of the restart)
        if let Some(at) = std::env::var("VELO_DEMO_LOSE").ok().and_then(|s| s.parse::<f64>().ok())
            && self.restarted.is_none()
            && now > at
            && !live.engine.status.lost.load(std::sync::atomic::Ordering::Relaxed)
        {
            live.engine.device().destroy();
        }
        let fresh = live.shared.borrow_mut().fresh.take();
        match fresh {
            Some(Ok(engine)) => {
                let (shared, was, blackout, sharing, pattern) = (live.shared.clone(), live.live_scene().map(String::from), live.blackout, live.sharing, live.pattern);
                let mut live = live::Live::new(engine, shared);
                live.share(sharing);
                live.audio.listen(live::Audio::first_input(&self.settings.audio));
                live.set_link(self.settings.link);
                live.set_blackout(blackout);
                live.set_pattern(pattern);
                if let Some(scene) = was.and_then(|id| self.session.project.scene(&id)) {
                    live.resume(scene);
                }
                self.live = Some(live);
                self.say(Tone::Error, tr!("The GPU stopped answering: Velo started its engine again"));
            }
            Some(Err(e)) => self.say(Tone::Error, tr!("The GPU stopped answering and didn't come back: {}", e)),
            None if live.broken() && self.restarted.is_none_or(|at| at.elapsed() > Duration::from_secs(10)) => {
                self.restarted = Some(Instant::now());
                live.shared.borrow_mut().requests.push(host::Request::Restart);
            }
            None => {}
        }
    }

    /// The autopilot, once a frame (`now` in seconds): on its turn (on bars a beat early, so the
    /// scene's own boundary lands on it; on time when the seconds are up) or on a drop, the next
    /// scene is launched, its own way, unless one is waiting.
    fn autopilot(&mut self, dt: f32, now: f64) {
        let a = self.settings.autopilot;
        let Some(live) = &self.live else { return };
        let beats = live.beats();
        // heard every frame, the autopilot on or not: switched on mid-track it must not take
        // the bass it never followed for one that just came back
        let dropped = self.pilot.dropped(live.sources()[BASS], dt, beats) && a.drops;
        // a scene launched by hand that takes no turns is the performer's: it stays
        if !a.on || self.held_by_hand() || self.session.project.scenes.len() < 2 || self.pilot.timed != a.timed {
            (self.pilot.next, self.pilot.timed) = (None, a.timed);
            return;
        }
        // where the turn is counted, how long a turn is, and how early it's taken
        let (at, period, early) = if a.timed { (now, f64::from(a.seconds.max(1)), 0.0) } else { (beats, f64::from(a.bars.max(1)) * 4.0, 1.0) };
        let next = *self.pilot.next.get_or_insert(if a.timed { now + period } else { (beats / period).floor() * period + period });
        if live.engine.status.armed().is_some() || (at < next - early && !dropped) {
            return;
        }
        // on the clock the next turn counts from this launch; on bars it stays on the grid
        self.pilot.next = Some(match (at >= next - early, a.timed) {
            (_, true) => now + period,
            (true, false) => next + period,
            (false, false) => next,
        });
        let random = a.random.then(|| (beats * 7919.0).fract().abs());
        if let Some(id) = next_turn(&self.session.project.scenes, live.live_scene(), random) {
            self.launch(&id);
        }
    }

    fn sync_live(&mut self, ctx: &egui::Context) {
        self.watch_engine(ctx.input(|i| i.time));
        self.autopilot(ctx.input(|i| i.stable_dt).min(0.1), ctx.input(|i| i.time));
        self.usage.sample(self.live.as_ref().map(|l| l.fps));
        let Some(live) = &mut self.live else { return };
        live.budget = self.usage.vram_budget;
        if let Some((scene, beats)) = self.session.morph.take() {
            live.morph(scene, beats);
        }
        let scene = self.session.active();
        // wanted where it shows: the floating card, behind the canvas, the split canvas' picture
        // (it hides the card: without it here, the picture beside or under the nodes froze while
        // an output window kept going), or an Output node's viewer
        let output_node = scene.is_some_and(|s| s.nodes.iter().any(|n| node_kind(&n.kind).is_some_and(|k| k.category == Category::Output)));
        let wanted = !self.settings.hide_preview || self.settings.preview_background || self.settings.split != Split::Off || output_node;
        let mut error = live.sync(ctx, &self.session.project, scene, wanted);
        // sharing that stopped (no NDI runtime, no DirectX 11): said, and off
        let unshared = live.engine.status.share_error.lock().ok().and_then(|mut e| e.take());
        if unshared.is_some() {
            live.sharing = None;
        }
        // what the GPU refused that no build caught: said, never a crash
        if let Some(e) = live.engine.status.gpu_error.lock().ok().and_then(|mut e| e.take()) {
            error.get_or_insert(e);
        }
        // the Audio In being tuned sets the detectors: the selected one, else the edited scene's
        // first, else the live scene's (one input, one analyzer)
        fn audio_in<'a>(s: Option<&'a velo_core::Scene>, selected: &[String]) -> Option<&'a velo_core::Node> {
            let mut ins = s?.nodes.iter().filter(|n| n.kind == velo_core::AUDIO && !n.off);
            ins.clone().find(|n| selected.contains(&n.id)).or_else(|| ins.next())
        }
        let detection = live::detection(audio_in(scene, &self.canvas.selected).or_else(|| audio_in(live.live_scene().and_then(|id| self.session.project.scene(id)), &[])));
        live.audio.tune(detection);
        if live.live_scene().is_some()
            && let Some((drop, launch)) = self.demo_drop.take()
        {
            if let Some(scene) = self.session.project.scene(&drop).filter(|_| launch) {
                live.launch(scene);
            }
            self.session.active_scene = Some(drop);
        }
        if let Some(e) = error {
            self.say(Tone::Error, tr!("A node couldn't be built: {}", e));
        }
        if let Some(e) = unshared {
            self.say(Tone::Error, tr!("Sharing stopped: {}", e));
        }
    }

    /// Where the output is, if anywhere.
    fn live_output(&self) -> Option<host::Target> {
        self.live.as_ref().and_then(|l| l.shared.borrow().output.clone())
    }

    /// The output as a floating card in the canvas' top-right corner (Figma-like); P hides it.
    /// The frame-time HUD over the canvas's top-left corner: the engine's rate, the render
    /// thread's frame times of the last few seconds against the canvas's budget, frames late
    /// for the output and stalls, and what the UI costs.
    fn hud(&mut self, ctx: &egui::Context, t: &Theme, view: egui::Rect) {
        let Some(live) = &self.live else { return };
        let s = &live.engine.status;
        let (fps, us, late, stalls) = (live.fps, s.frame_us.load(Relaxed), s.late.load(Relaxed), s.stalls.load(Relaxed));
        self.frame_times.push_back(us as f32 / 1000.0);
        while self.frame_times.len() > 240 {
            self.frame_times.pop_front();
        }
        let budget = 1000.0 / self.session.project.canvas.fps.max(1) as f32;
        let (ui_avg, ui_worst) = self.cpu.stats();
        let times = &self.frame_times;
        egui::Area::new(Id::unique("hud")).order(egui::Order::Middle).fixed_pos(view.left_top() + egui::vec2(t.pad, t.pad)).interactable(false).show(ctx, |ui| {
            Frame::new().fill(t.bg_panel).stroke(egui::Stroke::new(1.0, t.line)).corner_radius(t.radius_md).inner_margin(t.pad).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = t.gap;
                let line = |ui: &mut egui::Ui, text: String, color| ui.label(RichText::new(text).font(Theme::mono(t.text_xs)).color(color));
                ui.horizontal(|ui| {
                    line(ui, tr!("ENGINE").into(), t.fg_muted);
                    ui.label(RichText::new(format!("{fps:.0} fps")).font(Theme::mono(t.text_md)).color(t.fg_text));
                });
                // each frame's time, one pixel each from the right; the budget as a line
                let (rect, _) = ui.allocate_exact_size(egui::vec2(240.0, 48.0), egui::Sense::hover());
                let top = (budget * 2.0).max(times.iter().copied().fold(0.0, f32::max));
                let y = |ms: f32| rect.bottom() - (ms / top).min(1.0) * rect.height();
                let painter = ui.painter();
                painter.rect_filled(rect, t.radius_sm, t.bg_field);
                painter.hline(rect.x_range(), y(budget), egui::Stroke::new(1.0, t.warn.gamma_multiply(0.6)));
                let n = times.len();
                let points = times.iter().enumerate().map(|(i, &ms)| egui::pos2(rect.right() - (n - 1 - i) as f32, y(ms))).collect();
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.0, t.accent)));
                line(ui, tr!("frame {} ms · budget {} ms", format!("{:.2}", us as f32 / 1000.0), format!("{budget:.1}")), t.fg_text);
                line(ui, tr!("late {} · stalls {}", late, stalls), if late + stalls > 0 { t.warn } else { t.fg_muted });
                line(ui, tr!("UI {} ms · worst {} ms", format!("{:.1}", ui_avg * 1000.0), format!("{:.1}", ui_worst * 1000.0)), t.fg_muted);
            });
        });
    }

    fn preview_card(&mut self, ctx: &egui::Context, t: &Theme, view: egui::Rect) {
        let c = self.session.project.canvas;
        // a third of a narrow canvas at most, so the nodes keep room beside it
        let w = (view.width() * 0.32).clamp(168.0, 288.0);
        let h = (w * c.height as f32 / c.width.max(1) as f32).clamp(60.0, 400.0);
        let picture = self.live.as_ref().and_then(live::Live::picture);
        let (blackout, frozen, pattern) = self.live.as_ref().map_or((false, false, false), |l| (l.blackout, l.frozen, l.pattern));
        let tip = self.tip(tr!("Hide the preview"), "stage.preview");
        // the program while the selected scene is the live one, else that scene alone, with
        // its Launch button (or its countdown, once launched)
        let (scene, name) = self.session.active().map(|s| (s.id.clone(), s.name.clone())).unwrap_or_default();
        let on_air = self.live.as_ref().is_none_or(|l| l.live_scene() == Some(scene.as_str()));
        let armed = self.live.as_ref().and_then(|l| l.armed(&scene));
        let launch_tip = self.tip(tr!("Launch"), "stage.launch");
        // on air, its edge lights up on every beat
        let beat = if on_air && t.motion > 0.0 { self.live.as_ref().map_or(0.0, |l| l.clock()[1]) } else { 0.0 };
        let mut launch = false;
        let (mut hide, mut behind) = (false, self.settings.preview_background);
        // a little window: drag it anywhere, it stays where you left it
        let shown = egui::Area::new(Id::unique("preview-card")).order(egui::Order::Middle).default_pos(view.right_top() + egui::vec2(-t.pad, t.pad)).pivot(egui::Align2::RIGHT_TOP).movable(true).constrain_to(view).show(ctx, |ui| {
            let card = Frame::new().fill(t.bg_panel).stroke(egui::Stroke::new(1.0, t.line)).corner_radius(t.radius_md).shadow(ctx.global_style().visuals.popup_shadow).inner_margin(t.gap).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.gap, t.gap);
                ui.horizontal(|ui| {
                    ui.set_width(w);
                    ui.add_space(t.gap);
                    let (what, color) = if on_air { (tr!("LIVE"), t.accent) } else { (tr!("PREVIEW"), t.fg_muted) };
                    ui.label(RichText::new(what).font(Theme::mono(t.text_xs)).extra_letter_spacing(1.0).color(color));
                    ui.add(egui::Label::new(RichText::new(format!("· {name}")).small().color(t.fg_text)).truncate());
                    // what the audience sees instead, said
                    for (on, what, color) in [(blackout, tr!("BLACKOUT"), t.warn), (frozen, tr!("FROZEN"), t.accent), (pattern, tr!("TEST PATTERN"), t.warn)] {
                        if on {
                            ui.label(RichText::new(what).font(Theme::mono(t.text_xs)).color(color));
                        }
                    }
                    kit::right(ui, |ui| {
                        hide = kit::icon_button(ui, t, icons::X, &tip, false).clicked();
                        if kit::icon_button(ui, t, icons::PICTURE_IN_PICTURE, tr!("Show the preview behind the canvas"), behind).clicked() {
                            behind ^= true;
                        }
                        if on_air {
                            // the size only where the header has room for it beside the name
                            if w >= 240.0 {
                                ui.label(RichText::new(format!("{}×{}", c.width, c.height)).font(Theme::mono(t.text_xs)).color(t.fg_muted));
                            }
                        } else {
                            let r = kit::icon_button(ui, t, icons::PLAY, &launch_tip, armed.is_some());
                            launch = !self.learnable(ui, &r, &Target::LaunchSelected) && r.clicked();
                            if let Some(progress) = armed {
                                widgets::countdown(ui, t, progress);
                            }
                        }
                    });
                });
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                match picture {
                    Some(id) => egui::Image::new((id, rect.size())).corner_radius(t.radius_sm).paint_at(ui, rect),
                    None => {
                        ui.painter().rect_filled(rect, t.radius_sm, t.bg_field);
                        kit::fit_text(ui.painter(), rect.center(), egui::Align2::CENTER_CENTER, tr!("Nothing goes out yet"), t.small(), t.fg_muted, rect.width());
                    }
                }
            });
            if beat > 0.01 {
                let edge = card.response.rect;
                ui.painter().rect_stroke(edge, t.radius_md, egui::Stroke::new(1.5, t.accent.gamma_multiply(0.8 * beat)), egui::StrokeKind::Inside);
                ui.painter().rect_stroke(edge.expand(2.0), t.radius_md + 2, egui::Stroke::new(3.0, t.accent.gamma_multiply(0.15 * beat)), egui::StrokeKind::Outside);
            }
        });
        self.canvas.covered = Some(shown.response.rect);
        if hide {
            self.settings.hide_preview = true;
        }
        if launch {
            self.launch(&scene);
        }
        self.settings.preview_background = behind;
    }

    /// A Grid's settings shown as what they are: the grid's size and gaps, a map of its
    /// cells (each with the picture of the input that fills it), and the picked cell's input,
    /// fit and corners.
    fn grid_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, first: bool) {
        let Some(scene) = self.session.active().cloned() else { return };
        let Some(n) = scene.node(node).cloned() else { return };
        let get = |id: &str| n.params.get(id).copied();
        let (cols, rows) = (get("columns").map_or(2, |v| v[0].round() as u32).clamp(1, 4), get("rows").map_or(2, |v| v[0].round() as u32).clamp(1, 4));
        let (gap, gaps) = (get("gap").map_or(0.0, |v| v[0]).max(0.0), get("gaps").map_or(0, |v| v[0].round() as usize).min(2));
        let back = get("background").unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let cell_of = |c: usize| get(&format!("cell_{}", c + 1)).unwrap_or(if c < 8 { [(c + 1) as f32, 0.0, 0.0, 0.0] } else { [0.0; 4] });
        // what feeds each input: its node's id and name
        let feeds: Vec<Option<(String, String)>> = (0..8u8).map(|p| scene.wire_into(node, p).and_then(|e| scene.node(&e.from.0)).map(|f| (f.id.clone(), f.name.clone()))).collect();
        let mut sets: Vec<(String, [f32; 4], bool)> = Vec::new();
        let mut close = false;
        kit::section(ui, t, icons::GRID_FOUR, tr!("GRID"), first, |ui| {
            ui.add_space(t.gap);
            let w = (ui.available_width() - t.gap) / 2.0;
            ui.horizontal(|ui| {
                for (label, id, mut v) in [(tr!("COLUMNS"), "columns", cols), (tr!("ROWS"), "rows", rows)] {
                    let r = kit::scrub(ui, t, label, &mut v, 1..=4, w);
                    if r.changed() {
                        sets.push((id.into(), [v as f32, 0.0, 0.0, 0.0], true));
                    }
                    close |= r.drag_stopped() || r.lost_focus();
                }
            });
            ui.horizontal(|ui| {
                let mut px = gap.round() as u32;
                let r = kit::scrub(ui, t, tr!("GAP PX"), &mut px, 0..=200, w);
                if r.changed() {
                    sets.push(("gap".into(), [px as f32, 0.0, 0.0, 0.0], true));
                }
                close |= r.drag_stopped() || r.lost_focus();
                // the background beside it, in what's left of the row
                let (r, picked) = widgets::color(ui, t, tr!("Background"), back);
                r.on_hover_text(tr!("Background"));
                if let Some(c) = picked {
                    sets.push(("background".into(), c, true));
                }
            });
            kit::label(ui, t, tr!("Gaps"));
            if let Some(i) = kit::segmented(ui, t, gaps, &[tr!("Everywhere"), tr!("Inside"), tr!("Outside")]) {
                sets.push(("gaps".into(), [i as f32, 0.0, 0.0, 0.0], false));
            }
        });
        // the map: the canvas' shape, the cells as the shader lays them out
        let count = (cols * rows) as usize;
        let mut picked = self.canvas.grid_cell.min(count - 1);
        let canvas = self.session.project.canvas;
        let size = egui::vec2(canvas.width.max(1) as f32, canvas.height.max(1) as f32);
        kit::section(ui, t, icons::IMAGES, tr!("CELLS"), false, |ui| {
            ui.add_space(t.gap);
            let width = ui.available_width();
            let (map, _) = ui.allocate_exact_size(egui::vec2(width, width * size.y / size.x), egui::Sense::hover());
            let k = width / size.x;
            let back_color: egui::Color32 = egui::Rgba::from_rgb(back[0], back[1], back[2]).into();
            ui.painter().rect_filled(map, t.radius_sm, back_color);
            let outer = if gaps != 1 { gap } else { 0.0 } * k;
            let inner = if gaps != 2 { gap } else { 0.0 } * k;
            let grid = egui::vec2(cols as f32, rows as f32);
            let cell = ((map.size() - egui::Vec2::splat(2.0 * outer) - (grid - egui::Vec2::splat(1.0)) * inner) / grid).max(egui::vec2(4.0, 4.0));
            for c in 0..count {
                let (x, y) = ((c as u32 % cols) as f32, (c as u32 / cols) as f32);
                let r = egui::Rect::from_min_size(map.min + egui::vec2(outer + x * (cell.x + inner), outer + y * (cell.y + inner)), cell);
                let s = cell_of(c);
                let input = s[0].round() as usize;
                let corner = (s[2] * k).clamp(0.0, cell.min_elem() / 2.0);
                let source = input.checked_sub(1).and_then(|i| feeds.get(i)).and_then(|f| f.as_ref());
                match source.and_then(|(id, _)| self.live.as_ref().and_then(|l| l.thumb(id))) {
                    Some((tex, uv)) => egui::Image::new((tex, r.size())).uv(uv).corner_radius(corner).paint_at(ui, r),
                    None => {
                        ui.painter().rect_filled(r, corner, t.bg_field);
                    }
                }
                // the cell's number in its corner; in its middle, what it shows when nothing's drawn
                kit::fit_text(ui.painter(), r.left_top() + egui::vec2(6.0, 4.0), egui::Align2::LEFT_TOP, (c + 1).to_string(), Theme::mono(t.text_xs), t.fg_text, r.width() - 8.0);
                if source.is_none() {
                    let text = if input == 0 { tr!("None").to_string() } else { tr!("Input {}", input) };
                    kit::fit_text(ui.painter(), r.center(), egui::Align2::CENTER_CENTER, text, t.small(), t.fg_muted, r.width() - 8.0);
                }
                let resp = ui.interact(r, ui.scope_id().with(("grid cell", node, c)), egui::Sense::click()).on_hover_text(tr!("Cell {}", c + 1));
                if resp.clicked() {
                    picked = c;
                }
                let (width, color) = if c == picked {
                    (2.0, t.accent)
                } else if resp.hovered() {
                    (1.0, t.fg_muted)
                } else {
                    (1.0, t.line)
                };
                ui.painter().rect_stroke(r, corner, egui::Stroke::new(width, color), egui::StrokeKind::Inside);
            }
        });
        self.canvas.grid_cell = picked;
        // the picked cell
        let s = cell_of(picked);
        let id = format!("cell_{}", picked + 1);
        kit::section(ui, t, icons::IMAGE_SQUARE, &tr!("CELL {}", picked + 1), false, |ui| {
            ui.add_space(t.gap);
            let input = (s[0].round().max(0.0) as usize).min(8);
            let name = |i: usize| match i {
                0 => tr!("None").to_string(),
                i => feeds[i - 1].as_ref().map_or_else(|| tr!("{} · not wired", i), |f| format!("{i} · {}", f.1)),
            };
            let r = kit::select_named(ui, t, tr!("Input"), &name(input));
            kit::menu(t, &r).show(|ui| {
                let names: Vec<String> = (0..=8).map(name).collect();
                let items: Vec<(&str, &str)> = names.iter().map(|n| ("", n.as_str())).collect();
                if let Some(i) = kit::choose(ui, t, &items, Some(input)) {
                    sets.push((id.clone(), [i as f32, s[1], s[2], 0.0], false));
                }
            });
            let fit = (s[1].round().max(0.0) as usize).min(2);
            kit::label(ui, t, tr!("Fit"));
            if let Some(f) = kit::segmented(ui, t, fit, &[tr!("Fill"), tr!("Fit"), tr!("Stretch")]) {
                sets.push((id.clone(), [s[0], f as f32, s[2], 0.0], false));
            }
            let mut radius = s[2].round().max(0.0) as u32;
            let r = kit::scrub(ui, t, tr!("CORNERS PX"), &mut radius, 0..=200, ui.available_width());
            if r.changed() {
                sets.push((id.clone(), [s[0], s[1], radius as f32, 0.0], true));
            }
            close |= r.drag_stopped() || r.lost_focus();
        });
        for (param, value, merge) in sets {
            let props = json!({ "node": node, "param": param, "value": value });
            if merge {
                self.run_merge("node.set_param", props, &format!("param {node} {param}"));
            } else {
                self.run("node.set_param", props);
            }
        }
        if close {
            self.session.close_merge();
        }
    }

    /// An Image node's file: its name, what the engine made of it, a file or a folder to pick.
    fn media_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, first: bool) {
        let path = self.session.active().and_then(|s| s.node(node)).and_then(|n| n.media.clone());
        let video = self.session.active().and_then(|s| s.node(node)).is_some_and(|n| n.kind == velo_core::VIDEO);
        let loading = path.as_ref().and_then(|_| self.live.as_ref()?.media(node));
        let mut pick = None;
        let param = |id: &str, or: f32| self.session.active().and_then(|s| s.node(node)).and_then(|n| n.params.get(id)).map_or(or, |v| v[0]);
        let window = (param("loop_start", 0.0), param("loop_end", 1.0));
        let (mut looped, mut loop_done) = (None, false);
        kit::section(ui, t, icons::IMAGE, tr!("FILE"), first, |ui| {
            let Some(path) = &path else {
                // ProRes plays on macOS only
                let (icon, title, detail) = if video {
                    (icons::FILM_STRIP, tr!("Choose a movie"), if cfg!(target_os = "macos") { tr!("HAP, H.264, HEVC or ProRes, in .mov or .mp4") } else { tr!("HAP, H.264 or HEVC, in .mov or .mp4") })
                } else {
                    (icons::IMAGE, tr!("Choose a picture or a movie"), tr!("A still, an animated GIF or a HAP movie"))
                };
                if kit::drop_zone(ui, t, icon, title, detail).clicked() {
                    pick = Some(false);
                }
                if !video && kit::actions(ui, t, &[(kit::Variant::Ghost, Some(icons::FOLDER_OPEN), tr!("A folder of frames…"))])[0].clone().on_hover_text(tr!("A folder of numbered frames, played at 30 fps")).clicked() {
                    pick = Some(true);
                }
                return;
            };
            file_row(ui, t, if video { icons::FILM_STRIP } else { icons::IMAGE }, path);
            match loading.as_ref().map(|l| l.get()) {
                None | Some(None) => kit::hint(ui, t, tr!("Loading…")),
                Some(Some(Ok(m))) if m.movie.is_some() => {
                    let codec = m.movie.as_ref().map_or("", |mv| mv.format.label());
                    kit::hint(ui, t, &tr!("{} · {} frames · {}×{} · {} s loop", codec, m.count(), m.size[0], m.size[1], format!("{:.1}", m.length())))
                }
                Some(Some(Ok(m))) if m.count() > 1 => kit::hint(ui, t, &tr!("{} frames · {}×{} · {} s loop", m.count(), m.size[0], m.size[1], format!("{:.1}", m.length()))),
                Some(Some(Ok(m))) => kit::hint(ui, t, &tr!("One image · {}×{}", m.size[0], m.size[1])),
                Some(Some(Err(e))) => kit::warning(ui, t, e),
            };
            // a movie, a GIF, a sequence: which part of it loops, on its timeline
            if let Some(Some(Ok(m))) = loading.as_ref().map(|l| l.get())
                && m.count() > 1
            {
                ui.add_space(t.gap);
                kit::label(ui, t, tr!("Loop"));
                let (r, picked) = widgets::loop_range(ui, t, m.length(), window.0, window.1);
                looped = picked;
                loop_done = r.drag_stopped() || r.double_clicked();
            }
            ui.add_space(t.gap);
            let mut buttons = vec![(kit::Variant::Outline, Some(icons::ARROWS_CLOCKWISE), tr!("Replace…"))];
            if !video {
                buttons.push((kit::Variant::Outline, Some(icons::FOLDER_OPEN), tr!("Folder…")));
            }
            let shown = kit::actions(ui, t, &buttons);
            if shown[0].clone().on_hover_text(if video { tr!("A movie: .mov or .mp4") } else { tr!("An image, an animated GIF or a movie") }).clicked() {
                pick = Some(false);
            }
            if shown.get(1).is_some_and(|r| r.clone().on_hover_text(tr!("A folder of numbered frames, played at 30 fps")).clicked()) {
                pick = Some(true);
            }
        });
        let picked = match pick {
            Some(false) if video => rfd::FileDialog::new().add_filter(tr!("Movies"), &velo_engine::media::MOVIES).pick_file(),
            Some(false) => rfd::FileDialog::new().add_filter(tr!("Images"), &velo_engine::media::EXTENSIONS).pick_file(),
            Some(true) => rfd::FileDialog::new().pick_folder(),
            None => None,
        };
        if let Some(picked) = picked {
            self.run("node.set_media", json!({ "node": node, "media": picked.to_string_lossy() }));
        }
        // both ends of the loop, one undo step a drag
        if let Some((a, b)) = looped {
            let key = format!("loop {node}");
            self.run_merge("node.set_param", json!({ "node": node, "param": "loop_start", "value": a }), &key);
            self.run_merge("node.set_param", json!({ "node": node, "param": "loop_end", "value": b }), &key);
        }
        if loop_done {
            self.session.close_merge();
        }
    }

    /// A Media List's files in the order it plays them: each to move up or take out, more to
    /// add (up to [`velo_core::MEDIA_LIST_FILES`]), or all of them to take out.
    fn media_list_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str) {
        let Some(n) = self.session.active().and_then(|s| s.node(node)) else { return };
        let files: Vec<String> = velo_core::media_list(n).into_iter().map(String::from).collect();
        // each file the engine couldn't play on its last turn, and why
        let failed: Vec<Option<String>> = files.iter().map(|f| velo_engine::media::failed(f)).collect();
        let (mut act, mut add, mut clear) = (None, false, false);
        let max = velo_core::MEDIA_LIST_FILES;
        kit::section(ui, t, icons::IMAGES, tr!("FILES"), false, |ui| {
            if files.is_empty() {
                add = kit::drop_zone(ui, t, icons::IMAGES, tr!("Add pictures or movies"), &tr!("Up to {}, streamed in turn", max)).clicked();
                return;
            }
            for (k, path) in files.iter().enumerate() {
                let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.control_h), egui::Sense::hover());
                // inside the row: one past its edge widens the panel, and each row after it
                let button = |i: f32| egui::Rect::from_center_size(egui::pos2(row.right() - t.control_h / 2.0 - i * (t.control_h + 2.0), row.center().y), egui::vec2(t.control_h, t.control_h));
                if ui.put(button(0.0), |ui: &mut egui::Ui| kit::icon_button(ui, t, icons::X, tr!("Take it out"), false)).clicked() {
                    act = Some((k, false));
                }
                if k > 0 && ui.put(button(1.0), |ui: &mut egui::Ui| kit::icon_button_turned(ui, t, icons::CARET_DOWN, std::f32::consts::PI, tr!("Earlier"), false)).clicked() {
                    act = Some((k, true));
                }
                let name = std::path::Path::new(path).file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
                kit::fit_text(ui.painter(), egui::pos2(row.left(), row.center().y), egui::Align2::LEFT_CENTER, format!("{}", k + 1), Theme::mono(t.text_xs), t.fg_muted, 16.0);
                let color = if failed[k].is_some() { t.warn } else { t.fg_text };
                let (named, _) = kit::fit_text(ui.painter(), egui::pos2(row.left() + 18.0, row.center().y), egui::Align2::LEFT_CENTER, name, t.body(), color, button(1.0).left() - row.left() - 24.0);
                // why one doesn't show (its turn is black)
                if let Some(e) = &failed[k] {
                    ui.interact(named, egui::Id::unique(("list file", node, k)), egui::Sense::hover()).on_hover_text(e.as_str());
                }
            }
            ui.add_space(t.gap);
            // more files while there's room, or none at all (one step to undo)
            let w = ((ui.available_width() - t.gap) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.gap;
                add = ui.add_enabled_ui(files.len() < max, |ui| kit::action_sized(ui, t, kit::Variant::Outline, Some(icons::FILE_PLUS), tr!("Add files…"), Some(w))).inner.clicked();
                clear = kit::action_sized(ui, t, kit::Variant::Outline, Some(icons::TRASH), tr!("Remove all"), Some(w)).clicked();
            });
            kit::hint(ui, t, &tr!("{} of {} files", files.len(), max));
        });
        let mut list = files.clone();
        match act {
            Some((k, true)) => list.swap(k, k - 1),
            Some((k, false)) => {
                list.remove(k);
            }
            None => {}
        }
        if clear {
            list.clear();
        }
        if add && let Some(picked) = rfd::FileDialog::new().add_filter(tr!("Pictures and movies"), &velo_engine::media::EXTENSIONS).pick_files() {
            list.extend(picked.iter().map(|p| p.to_string_lossy().into_owned()));
            list.truncate(max);
        }
        if list != files {
            self.run("node.set_media", json!({ "node": node, "media": list.join("\n") }));
        }
    }

    /// A Mesh node's model: the .obj or .ply it draws, and why it can't draw it (no faces, or
    /// more triangles than it takes).
    fn mesh_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str) {
        let path = self.session.active().and_then(|s| s.node(node)).and_then(|n| n.media.clone());
        let loading = path.as_ref().and_then(|_| self.live.as_ref()?.media(node));
        let failed = loading.as_ref().and_then(|l| l.get()?.as_ref().err().cloned());
        // how many triangles the file has, and how many corners the mesh got of them
        let faces = loading.as_ref().and_then(|l| l.get()?.as_ref().ok()?.model.as_ref().map(|m| (m.faces, m.mesh.len())));
        let mut pick = false;
        kit::section(ui, t, icons::POLYGON, tr!("MODEL"), false, |ui| {
            let Some(path) = &path else {
                pick = kit::drop_zone(ui, t, icons::POLYGON, tr!("Choose a 3D model"), tr!("An .obj or .ply with faces")).clicked();
                return;
            };
            file_row(ui, t, icons::POLYGON, path);
            match (&failed, faces) {
                (Some(e), _) => kit::warning(ui, t, e),
                (None, Some((0, _))) => kit::warning(ui, t, tr!("It has no faces, only points: a Particles node can be born on them.")),
                (None, Some((n, 0))) => kit::warning(ui, t, &tr!("{} triangles: a Mesh draws {} at most.", n, velo_engine::model::MAX_TRIANGLES)),
                (None, _) => kit::hint(ui, t, tr!("Drawn solid and lit by a light; the camera settings turn it round. One mesh: no textures, shadows or animation.")),
            };
            ui.add_space(t.gap);
            pick = kit::actions(ui, t, &[(kit::Variant::Outline, Some(icons::ARROWS_CLOCKWISE), tr!("Replace…"))])[0].clicked();
        });
        if let Some(picked) = pick.then(|| rfd::FileDialog::new().add_filter(tr!("3D models"), &velo_engine::model::EXTENSIONS).pick_file()).flatten() {
            self.run("node.set_media", json!({ "node": node, "media": picked.to_string_lossy() }));
        }
    }

    /// A Live node's source: the NDI sources and Spout senders there are right now, to pick from
    /// (the list looks again by itself every few seconds), and why one can't be reached.
    fn live_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str) {
        let current = self.session.active().and_then(|s| s.node(node)).and_then(|n| n.media.clone());
        let failed = current.as_ref().and_then(|_| self.live.as_ref()?.media(node)).and_then(|l| l.get()?.as_ref().err().cloned());
        let mut sources = velo_engine::input::sources();
        // the one chosen stays in the list, found or not
        if let Some(chosen) = current.as_ref().filter(|c| !sources.contains(c)) {
            sources.insert(0, chosen.clone());
        }
        let mut pick = None;
        kit::section(ui, t, icons::BROADCAST, tr!("LIVE SOURCE"), false, |ui| {
            match (&failed, sources.is_empty()) {
                (Some(e), _) => kit::warning(ui, t, e),
                (None, true) => kit::hint(ui, t, tr!("Looking for sources…")),
                (None, false) => kit::hint(ui, t, tr!("Pick an NDI source, a Spout sender or a camera: what it sends, as it sends it.")),
            };
            ui.add_space(t.gap);
            // one list to choose from: none, then what's there
            let r = kit::select(ui, t, Some(icons::BROADCAST), current.as_deref().unwrap_or(tr!("None")));
            kit::menu(t, &r).show(|ui| {
                let items: Vec<(&str, &str)> = std::iter::once(("", tr!("None"))).chain(sources.iter().map(|s| ("", s.as_str()))).collect();
                let chosen = current.as_ref().and_then(|c| sources.iter().position(|s| s == c)).map_or(0, |i| i + 1);
                match kit::choose(ui, t, &items, Some(chosen)) {
                    Some(0) => pick = Some(String::new()),
                    Some(i) => pick = Some(sources[i - 1].clone()),
                    None => {}
                }
            });
        });
        if let Some(source) = pick {
            self.run("node.set_media", json!({ "node": node, "media": source }));
        }
        // the list fills in as the look finishes
        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    }

    /// A Particles node's sprite: the picture each particle is (a still, a GIF, a movie), or
    /// soft dots without one; or a 3D model (.obj, .ply) they are born on, shape Model.
    fn sprite_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str) {
        let path = self.session.active().and_then(|s| s.node(node)).and_then(|n| n.media.clone());
        let failed = path.as_ref().and_then(|_| self.live.as_ref()?.media(node)).and_then(|l| l.get()?.as_ref().err().cloned());
        let (mut pick, mut clear) = (false, false);
        kit::section(ui, t, icons::SPARKLE, tr!("SPRITE"), false, |ui| {
            let Some(path) = &path else {
                pick = kit::drop_zone(ui, t, icons::IMAGE, tr!("Choose a picture or a 3D model"), tr!("Each particle becomes a picture, tinted by Tint; a .obj or .ply is what they're born on")).clicked();
                return;
            };
            file_row(ui, t, icons::IMAGE, path);
            match (&failed, velo_engine::model::is_model(std::path::Path::new(path))) {
                (Some(e), _) => kit::warning(ui, t, e),
                (None, true) => kit::hint(ui, t, tr!("The particles are born on its points: set Shape to Model, and View to Orbit to turn a camera round it.")),
                (None, false) => kit::hint(ui, t, tr!("Each particle draws it, tinted by Tint. Stills, GIFs and movies work.")),
            };
            ui.add_space(t.gap);
            let shown = kit::actions(ui, t, &[(kit::Variant::Outline, Some(icons::ARROWS_CLOCKWISE), tr!("Replace…")), (kit::Variant::Ghost, Some(icons::SPARKLE), tr!("Soft dots"))]);
            pick = shown[0].clicked();
            clear = shown[1].clicked();
        });
        if let Some(picked) = pick.then(|| rfd::FileDialog::new().add_filter(tr!("Images"), &velo_engine::media::EXTENSIONS).add_filter(tr!("3D models"), &velo_engine::model::EXTENSIONS).pick_file()).flatten() {
            self.run("node.set_media", json!({ "node": node, "media": picked.to_string_lossy() }));
        }
        if clear {
            self.run("node.set_media", json!({ "node": node, "media": "" }));
        }
    }

    /// A Text node's own font file: its name, why it doesn't set if it can't, a file to pick
    /// (and Font set to File, in the same undo step).
    fn font_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, path: Option<&str>) {
        let failed = path.and_then(|_| self.live.as_ref()?.media(node)).and_then(|l| l.get()?.as_ref().err().cloned());
        let mut pick = false;
        kit::section(ui, t, icons::TEXT_AA, tr!("FONT FILE"), false, |ui| {
            let Some(path) = path else {
                pick = kit::drop_zone(ui, t, icons::TEXT_AA, tr!("Choose a font"), tr!("A .ttf or .otf file")).clicked();
                return;
            };
            file_row(ui, t, icons::TEXT_AA, path);
            if let Some(e) = &failed {
                kit::warning(ui, t, e);
            }
            ui.add_space(t.gap);
            pick = kit::actions(ui, t, &[(kit::Variant::Outline, Some(icons::ARROWS_CLOCKWISE), tr!("Replace…"))])[0].clicked();
        });
        if let Some(picked) = pick.then(|| rfd::FileDialog::new().add_filter(tr!("Fonts"), &["ttf", "otf"]).pick_file()).flatten() {
            let key = format!("font {node}");
            self.run_merge("node.set_media", json!({ "node": node, "media": picked.to_string_lossy() }), &key);
            self.run_merge("node.set_param", json!({ "node": node, "param": "font", "value": 2.0 }), &key);
            self.session.close_merge();
        }
    }

    /// A node's parameters, from its shader's header: sliders, switches, colors. Values go
    /// through `node.set_param`; a drag is one undo step. A source dropped on a slider maps it
    /// (`mapping.add`); its badges open the mapping's settings; the band and the live dot show
    /// what the engine does with it.
    fn node_params(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, first: bool) {
        let Some(n) = self.session.project.scenes.iter().find_map(|s| s.node(node)).cloned() else { return };
        // an Audio In: its input first, the top bar's own (one input for now)
        let audio = n.kind == velo_core::AUDIO;
        if audio {
            kit::section(ui, t, icons::MICROPHONE, tr!("INPUT"), first, |ui| self.audio_source(ui, t));
        }
        // a Text node: its words first (each line set again when they change), its own font
        // file; the rest are its parameters. A Text List's items, one for each turn.
        let text = velo_core::sets_text(&n.kind);
        if n.kind == velo_core::TEXT_LIST {
            self.items_section(ui, t, node, n.text.as_deref().unwrap_or_default(), first);
        } else if text {
            let mut words = n.text.clone().unwrap_or_default();
            kit::section(ui, t, icons::TEXT_T, tr!("TEXT"), first, |ui| {
                let r = ui.add(egui::TextEdit::multiline(&mut words).desired_rows(2).desired_width(f32::INFINITY).font(t.body()));
                if r.changed() {
                    self.run_merge("node.set_text", json!({ "node": node, "text": words }), &format!("text {node}"));
                }
                if r.lost_focus() {
                    self.session.close_merge();
                }
            });
            // its own font file, once Font is set to File (or one is chosen)
            if n.media.is_some() || n.params.get("font").is_some_and(|v| v[0].round() == 2.0) {
                self.font_section(ui, t, node, n.media.as_deref());
            }
        }
        self.shader_params(ui, t, node, first && !audio && !text);
        let signal = self.session.active().and_then(|s| s.node(node)).and_then(|n| node_kind(&n.kind)).is_some_and(|k| k.category == Category::Signal);
        if signal {
            self.signal_links(ui, t, node);
        }
    }

    /// A Text List's items, one for each turn: a field each (Enter starts another line of the same
    /// item), × to take one out, + to add one, or all of them from a file. Typing in one is one
    /// undo step, as in a Text.
    fn items_section(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, words: &str, first: bool) {
        let mut items: Vec<String> = velo_core::text_items(words).into_iter().map(String::from).collect();
        let (mut typed, mut done, mut remove, mut add, mut import) = (false, false, None, false, false);
        kit::section(ui, t, icons::LIST_BULLETS, tr!("ITEMS"), first, |ui| {
            let count = items.len();
            for (k, item) in items.iter_mut().enumerate() {
                ui.horizontal_top(|ui| {
                    // its number, its field, x on the right
                    let (number, _) = ui.allocate_exact_size(egui::vec2(16.0, t.control_h), egui::Sense::hover());
                    ui.painter().text(number.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), Theme::mono(t.text_xs), t.fg_muted);
                    let width = ui.available_width() - t.control_h - t.gap;
                    let field = egui::TextEdit::multiline(item).desired_rows(1).desired_width(width).font(t.body()).margin(egui::Margin::symmetric(8, 6)).hint_text(tr!("Words for this turn"));
                    let r = ui.add(field);
                    (typed, done) = (typed || r.changed(), done || r.lost_focus());
                    let x = ui.add_enabled_ui(count > 1, |ui| kit::icon_button(ui, t, icons::X, tr!("Take this item out"), false)).inner;
                    if x.clicked() {
                        remove = Some(k);
                    }
                });
            }
            ui.horizontal(|ui| {
                add = kit::button(ui, t, Some(icons::PLUS), tr!("Add an item"), false).on_hover_text(tr!("Another turn of words: it shows them one at a time, as its Order, Clock and Every say")).clicked();
                import = kit::button(ui, t, Some(icons::FILE_PLUS), tr!("From a file…"), false).on_hover_text(tr!("Items from a .txt (a line each, or a block between empty lines) or a .csv (a row each, its cells on lines): they replace these")).clicked();
            });
        });
        if import {
            return self.import_items_dialog(node);
        }
        if let Some(k) = remove {
            items.remove(k);
        }
        if add {
            items.push(String::new());
        }
        let props = json!({ "node": node, "text": velo_core::join_items(&items) });
        if remove.is_some() || add {
            // taking one out or adding one: its own undo step
            self.session.close_merge();
            self.run("node.set_text", props);
        } else if typed {
            self.run_merge("node.set_text", props, &format!("text {node}"));
        }
        if done {
            self.session.close_merge();
        }
    }

    /// A generator's settings thrown somewhere new, as one undo step: each number most of the
    /// way to a random place along its slider, a choice or a switch picked anew. Colors (the
    /// background stays as it is), Automagic, opacity, how many particles and what a macro
    /// drives keep their values.
    fn randomize(&mut self, node: &str, shader: &live::Shader, set: &std::collections::BTreeMap<String, [f32; 4]>, scene: &Scene) {
        use velo_engine::ParamType;
        let mut x = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 40) as f32 / (1u64 << 24) as f32
        };
        let key = format!("random {node}");
        for (param, infos) in &shader.controls {
            if matches!(param.ty, ParamType::Color | ParamType::Vec4) || live::automagic(&param.id) || ["opacity", "count", "font", "inputs"].contains(&param.id.as_str()) {
                continue;
            }
            let mut value = set.get(&param.id).copied().unwrap_or(param.default);
            for (axis, info) in infos.iter().enumerate() {
                let key = if param.ty == ParamType::Vec2 { format!("{}.{}", param.id, ["x", "y"][axis]) } else { param.id.clone() };
                if scene.driver(node, &key).is_some() {
                    continue;
                }
                value[axis] = match param.ty {
                    ParamType::Bool => f32::from(u8::from(next() < 0.5)),
                    ParamType::Int if !param.names.is_empty() => (next() * param.names.len() as f32).floor().min(param.names.len() as f32 - 1.0),
                    _ => info.from_norm(info.to_norm(value[axis]) + (next() - info.to_norm(value[axis])) * 0.7),
                };
            }
            self.run_merge("node.set_param", json!({ "node": node, "param": param.id, "value": value }), &key);
        }
        self.session.close_merge();
    }

    /// What a signal node's outputs move, each mapping with its settings as the moved
    /// parameter's badge has them: changed from either end.
    fn signal_links(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str) {
        let Some(scene) = self.session.active().cloned() else { return };
        let Some(outputs) = scene.node(node).and_then(|n| node_kind(&n.kind)).map(|k| k.outputs) else { return };
        let links: Vec<(&velo_core::Node, &Mapping, u8)> =
            scene.nodes.iter().flat_map(|n| n.mappings.iter().map(move |m| (n, m))).filter_map(|(n, m)| velo_core::signal_output(&m.source).filter(|(id, _)| *id == node).map(|(_, port)| (n, m, port))).collect();
        let mut ops = Vec::new();
        let mut close = false;
        // which outputs its card lists (one in use always shows)
        if outputs.len() > 1 {
            let hidden = scene.node(node).and_then(|n| n.params.get("hidden")).map_or(0, |v| v[0] as u32);
            kit::section(ui, t, icons::EYE, tr!("ON ITS CARD"), false, |ui| {
                for (p, name) in outputs.iter().enumerate() {
                    let used = canvas::output_used(&scene, node, p as u8);
                    let info = ParamInfo { name: source_label(name), min: 0.0, max: 1.0, default: 1.0, curve: velo_core::Curve::Linear, unit: "onoff" };
                    let on = hidden & (1 << p) == 0 || used;
                    let shown = ui.add_enabled_ui(!used, |ui| widgets::toggle(ui, t, &info, f32::from(u8::from(on))).1).inner;
                    if let Some(v) = shown {
                        let mask = if v >= 0.5 { hidden & !(1 << p) } else { hidden | (1 << p) };
                        ops.push(("node.set_param", json!({ "node": node, "param": "hidden", "value": mask as f32 }), None));
                    }
                }
            });
        }
        kit::section(ui, t, icons::PATH, tr!("MOVES"), false, |ui| {
            if links.is_empty() {
                kit::hint(ui, t, tr!("Nothing yet. Drag from one of its outputs onto a node to choose what it moves."));
            }
            // under the output each comes from: its name once, then a row for each thing it moves
            let mut by_output = links.clone();
            by_output.sort_by_key(|l| l.2);
            let mut heading = None;
            ui.spacing_mut().item_spacing.y = 3.0;
            for (target, m, port) in &by_output {
                // "Kick → Kaleido · Zoom", the parameter as its inspector names it
                let param_name = param_name(target, &m.param);
                let from = if outputs.len() > 1 { outputs.get(usize::from(*port)).map_or("?", |o| source_label(o)) } else { tr!("Out") };
                if heading != Some(*port) {
                    if heading.is_some() {
                        ui.add_space(t.gap);
                    }
                    heading = Some(*port);
                    let (row, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
                    ui.painter().circle_filled(row.left_center() + egui::vec2(6.0, 0.0), 3.0, t.cat_signal);
                    ui.painter().text(row.left_center() + egui::vec2(16.0, 0.0), egui::Align2::LEFT_CENTER, from.to_uppercase(), Theme::mono(t.text_xs), t.fg_text);
                    let count = by_output.iter().filter(|l| l.2 == *port).count();
                    ui.painter().text(row.right_center() - egui::vec2(4.0, 0.0), egui::Align2::RIGHT_CENTER, count.to_string(), Theme::mono(t.text_xs), t.fg_muted);
                }
                let r = widgets::move_row(ui, t, &target.name, &param_name, m.amount);
                if let Some(edit) = widgets::mapping_popup(t, &r, &format!("{from} → {} · {param_name}", target.name), m) {
                    map_ops(&edit, &target.id, m, &mut ops);
                    close |= edit.done;
                }
            }
        });
        for (op, mut props, merge) in ops {
            props["scene"] = json!(scene.id);
            match merge {
                Some(key) => self.run_merge(op, props, &key),
                None => self.run(op, props),
            }
        }
        if close {
            self.session.close_merge();
        }
    }

    fn shader_params(&mut self, ui: &mut egui::Ui, t: &Theme, node: &str, first: bool) {
        let Some(n) = self.session.project.scenes.iter().find_map(|s| s.node(node)).cloned() else { return };
        let (kind, set, mappings, code) = (n.kind.as_str(), &n.params, n.mappings.as_slice(), n.code.as_deref());
        let Some(shader) = live::shader(kind, code) else {
            let text = if velo_core::is_shader(kind) {
                tr!("Its parameters appear once the code's header is valid.")
            } else if kind == velo_core::CLOCK {
                tr!("Follows the tempo in the top bar: a hit on every beat and a ramp through each bar. Drag an output onto a node to choose what it moves.")
            } else {
                tr!("Coming soon: this node doesn't render yet, so what it feeds sees black.")
            };
            kit::section(ui, t, icons::SLIDERS_HORIZONTAL, tr!("PARAMETERS"), first, |ui| {
                kit::hint(ui, t, text);
            });
            return;
        };
        use velo_engine::ParamType;
        // an Audio In: what each detector watches, drawn over its sliders against this node's
        // thresholds; in Model mode the model's, and no band sliders
        let now = ui.input(|i| i.time);
        let audio = (kind == velo_core::AUDIO).then(|| {
            let graphs = self.live.as_mut().map(|l| {
                l.audio.sample_graphs(now);
                (l.audio.graphs.clone(), l.audio.gated)
            });
            let (graphs, gated) = graphs.unwrap_or_default();
            (graphs, live::detection(Some(&n)), gated)
        });
        // a still picture doesn't play: no speed for it
        let still = velo_core::shows_media(kind) && self.live.as_ref().and_then(|l| l.media(node)).is_some_and(|l| l.get().is_some_and(|r| r.as_ref().is_ok_and(|m| m.movie.is_none() && m.count() <= 1)));
        let (mut edits, mut slides, mut close, mut maps_ops) = (Vec::new(), Vec::new(), false, Vec::new());
        // what the sources read now (the badges glow with them), and the values the engine
        // computed for this node (the live dots)
        let sources = self.live.as_ref().map(live::Live::sources).unwrap_or_default();
        let level = |source: &str| match velo_core::signal_output(source) {
            Some((id, port)) => self.live.as_ref().and_then(|l| l.signal(id, port)).unwrap_or(0.0),
            None => SOURCES.iter().position(|s| *s == source).map_or(0.0, |i| sources[i]),
        };
        let modulated = if mappings.is_empty() { None } else { self.live.as_ref().and_then(|l| l.modulated(node)) };
        // the scene's macros (a driven slider shows its macro's doing) and what MIDI drives
        let Some(scene) = self.session.active().cloned() else { return };
        let names: Vec<String> = scene.macros.iter().enumerate().map(|(i, m)| midi::macro_name(m, i)).collect();
        let (learn_on, waiting) = (self.learn.on, self.learn.target.clone());
        // an empty macro waiting for a value: the first one touched is given to it
        let (assigning, mut assigned) = (self.assigning.as_ref().filter(|a| a.0 == scene.id).map(|a| a.1), None);
        let bound = |key: &str| self.binding_label(&Target::Param { node: node.to_string(), param: key.to_string() });
        let bound: Vec<(String, String)> =
            shader.controls.iter().flat_map(|(p, infos)| (0..infos.len()).map(move |a| if p.ty == ParamType::Vec2 { format!("{}.{}", p.id, ["x", "y"][a]) } else { p.id.clone() })).filter_map(|k| Some((k.clone(), bound(&k)?))).collect();
        let (mut learn, mut forget, mut asks) = (None, None, Vec::new());
        // what makes or changes a picture (not a file's) can be thrown somewhere new
        let random = node_kind(kind).is_some_and(|k| matches!(k.category, Category::Generator | Category::Effect | Category::Composite)) && !velo_core::shows_media(kind) && kind != velo_core::MEDIA_LIST;
        let (mut shuffle, mut reset) = (false, false);
        // what's set apart from how the node came
        let changed = shader.controls.iter().any(|(p, _)| set.get(&p.id).is_some_and(|v| *v != p.default));
        kit::section(ui, t, icons::SLIDERS_HORIZONTAL, tr!("PARAMETERS"), first, |ui| {
            ui.horizontal(|ui| {
                if random {
                    let r = kit::button(ui, t, Some(icons::SHUFFLE), tr!("Random"), false);
                    shuffle = r.clone().on_hover_text(tr!("Its settings somewhere new, colors and Automagic aside: click again for another, Ctrl+Z goes back")).clicked();
                }
                let r = ui.add_enabled_ui(changed, |ui| kit::button(ui, t, Some(icons::ARROW_CLOCKWISE), tr!("Reset"), false)).inner;
                reset = r.on_hover_text(tr!("Every setting back to how the node came: Ctrl+Z goes back")).clicked();
            });
            ui.add_space(t.gap);
            let mut switches = Vec::new();
            // Automagic first: what the node does with the music on its own
            let magic = shader.controls.iter().position(|(p, _)| live::automagic(&p.id));
            // then what kind of thing it does (a mode, a shape: its choices), then how much
            let choice = |i: &usize| shader.controls[*i].0.ty == ParamType::Int && !shader.controls[*i].0.names.is_empty();
            let rest = (0..shader.controls.len()).filter(|&i| Some(i) != magic);
            let order = magic.into_iter().chain(rest.clone().filter(choice)).chain(rest.filter(|i| !choice(i)));
            for index in order {
                let (param, infos) = &shader.controls[index];
                // the loop's part is the file's timeline, not two sliders
                if (still && param.id == "speed") || param.id == "loop_start" || param.id == "loop_end" {
                    continue;
                }
                // a Sequencer's steps past the ones it plays
                if kind == velo_core::SEQUENCER && param.id.strip_prefix("step_").and_then(|i| i.parse::<f32>().ok()).is_some_and(|i| i > set.get("steps").map_or(8.0, |v| v[0])) {
                    continue;
                }
                if let Some((graphs, detection, gated)) = &audio {
                    let model = detection.mode == velo_audio::Mode::Model;
                    if let Some(hit) = param.id.strip_suffix("_threshold").and_then(|h| velo_audio::HITS.iter().position(|x| *x == h)) {
                        // its line dragged: the threshold the mode reads, as its slider would
                        if let Some(v) = hit_graph(ui, t, hit, &graphs[hit], detection, *gated, now) {
                            let key = format!("{}_{}", velo_audio::HITS[hit], if model { "confidence" } else { "threshold" });
                            if let Some((_, infos)) = shader.controls.iter().find(|(p, _)| p.id == key) {
                                slides.push((key, infos[0].clamp(v)));
                            }
                        }
                    }
                    // Model mode shows each hit's threshold alone; Simple mode everything else
                    if velo_audio::HITS.iter().any(|h| param.id.starts_with(&format!("{h}_"))) && model != param.id.ends_with("_confidence") {
                        continue;
                    }
                }
                let value = set.get(&param.id).copied().unwrap_or(param.default);
                match param.ty {
                    ParamType::Bool => switches.push((param, &infos[0], value)),
                    // an int with named values: a segmented control, or a menu past three
                    ParamType::Int if !param.names.is_empty() => {
                        // driven by a macro: where the macro has it
                        let driver = scene.driver(node, &param.id).map(|(i, x)| (i, x.at(scene.macros[i].value)));
                        let value = driver.map_or(value, |(_, v)| [v, 0.0, 0.0, 0.0]);
                        let choices: Vec<&str> = param.names.iter().map(|n| i18n::t(n)).collect();
                        let at = (value[0].round().max(0.0) as usize).min(choices.len() - 1);
                        let (picked, r) = if kind == velo_core::AUDIO && param.id == "mode" {
                            // how it hears the drums: each way said, not two words on a switch
                            kit::label(ui, t, tr!("How it hears the drums"));
                            let says = [
                                (icons::SLIDERS_HORIZONTAL, tr!("A hit is a band rising past its threshold: you set each band and its line.")),
                                (icons::ROBOT, tr!("A drum model trained on full mixes hears kick, snare and hat by itself.")),
                            ];
                            let items: Vec<(&str, &str, &str)> = choices.iter().zip(says).map(|(name, (icon, what))| (icon, *name, what)).collect();
                            widgets::options(ui, t, at, &items)
                        } else if choices.len() <= 3 {
                            kit::label(ui, t, i18n::t(infos[0].name));
                            kit::segmented_with(ui, t, at, &choices)
                        } else {
                            let r = kit::select_named(ui, t, i18n::t(infos[0].name), choices[at]);
                            let mut picked = None;
                            if !learn_on {
                                kit::menu(t, &r).show(|ui| {
                                    let items: Vec<(&str, &str)> = choices.iter().map(|c| ("", *c)).collect();
                                    picked = kit::choose(ui, t, &items, Some(at));
                                });
                            }
                            (picked, r)
                        };
                        let target = Target::Param { node: node.to_string(), param: param.id.clone() };
                        if learn_on || waiting.as_ref() == Some(&target) {
                            widgets::learn_ring(ui, t, r.rect, waiting.as_ref() == Some(&target));
                        }
                        if learn_on {
                            if r.clicked() || picked.is_some() {
                                learn = Some(target.clone());
                            }
                        } else if let Some(i) = picked {
                            // a driven choice moves its macro there, as a driven slider does
                            match driver {
                                Some(_) => slides.push((param.id.clone(), i as f32)),
                                None => edits.push((param.id.clone(), [i as f32, 0.0, 0.0, 0.0], false)),
                            }
                        }
                        if let Some((i, _)) = driver {
                            kit::hint(ui, t, &tr!("{} drives it", names[i]));
                        }
                        let binding = bound.iter().find(|(k, _)| *k == param.id).map(|(_, b)| b.as_str());
                        let reset = driver.is_none() && value[0] != param.default[0];
                        if let Some(ask) = param_menu(t, &r, reset, Some((waiting.as_ref() == Some(&target), binding)), Some((&names, driver.map(|d| d.0)))) {
                            asks.push((ask, param.id.clone(), param.default, [infos[0].min, infos[0].max]));
                        }
                        ui.add_space(t.gap);
                    }
                    ParamType::Color => {
                        let name = i18n::t(infos[0].name);
                        let (r, picked) = widgets::color(ui, t, name, value);
                        if let Some(c) = picked {
                            edits.push((param.id.clone(), c, true));
                        }
                        close |= r.clicked();
                        // (a macro's range is a number: it never drives a color)
                        if let Some(ask) = param_menu(t, &r, value != param.default, None, None) {
                            asks.push((ask, param.id.clone(), param.default, [0.0, 1.0]));
                        }
                        // what turns its hue (a signal dropped on the node's card, picked as "Tint (hue)")
                        for m in mappings.iter().filter(|m| m.param == param.id) {
                            let title = format!("{} → {}", mapping_source(&scene, &m.source), tr!("{} (hue)", i18n::t(infos[0].name)));
                            let r = kit::button(ui, t, None, &format!("{title}   {:+.0}%", m.amount * 100.0), false);
                            if let Some(edit) = widgets::mapping_popup(t, &r, &title, m) {
                                map_ops(&edit, node, m, &mut maps_ops);
                                close |= edit.done;
                            }
                        }
                        ui.add_space(t.gap);
                    }
                    _ => {
                        // a small whole number nothing moves (how many inputs a Blend takes):
                        // picked from a row of cells, one a value
                        let moved = mappings.iter().any(|m| m.param == param.id) || scene.driver(node, &param.id).is_some() || bound.iter().any(|(k, _)| *k == param.id);
                        if param.ty == ParamType::Int && infos[0].max - infos[0].min <= 15.0 && !moved && !learn_on && assigning.is_none() {
                            let (r, picked) = widgets::count(ui, t, &infos[0], value[0]);
                            if let Some(v) = picked {
                                edits.push((param.id.clone(), [v, 0.0, 0.0, 0.0], false));
                            }
                            let target = Target::Param { node: node.to_string(), param: param.id.clone() };
                            if let Some(ask) = param_menu(t, &r, value[0] != param.default[0], Some((waiting.as_ref() == Some(&target), None)), Some((&names, None))) {
                                asks.push((ask, param.id.clone(), param.default, [infos[0].min, infos[0].max]));
                            }
                            continue;
                        }
                        for (axis, info) in infos.iter().enumerate() {
                            // what mappings call it: a vec2's axes are "offset.x" and "offset.y"
                            let key = if param.ty == ParamType::Vec2 { format!("{}.{}", param.id, ["x", "y"][axis]) } else { param.id.clone() };
                            let maps: Vec<&Mapping> = mappings.iter().filter(|m| m.param == key).collect();
                            let labels: Vec<String> = maps.iter().map(|m| mapping_source(&scene, &m.source)).collect();
                            let views: Vec<ModView> = maps.iter().zip(&labels).map(|(m, name)| ModView { source: name, amount: m.amount, level: level(&m.source) }).collect();
                            let live = modulated.filter(|_| !maps.is_empty()).and_then(|v| v.get(index * 2 + axis).copied());
                            // driven by a macro: where the macro has it, the range it takes it through
                            let driver = scene.driver(node, &key).map(|(i, x)| (i, x.clone()));
                            let shown = driver.as_ref().map_or(value[axis], |(i, x)| info.clamp(x.at(scene.macros[*i].value)));
                            let label = driver.as_ref().map(|(i, _)| names[*i].clone());
                            let drawn = driver.as_ref().zip(label.as_deref()).map(|((_, x), label)| widgets::Driver { label, range: [x.min, x.max] });
                            let binding = bound.iter().find(|(k, _)| *k == key).map(|(_, b)| b.as_str());
                            let magic = live::automagic(&param.id);
                            let out = widgets::slider(ui, t, info, shown, &widgets::Marks { mods: &views, live, driver: drawn, binding, icon: magic.then_some(icons::SPEAKER_HIGH), tint: None });
                            if magic {
                                out.response.clone().on_hover_text(tr!("Automagic: how much this node plays along with the music by itself, no mapping needed. At 0 it keeps still"));
                            }
                            let target = Target::Param { node: node.to_string(), param: key.clone() };
                            if learn_on || waiting.as_ref() == Some(&target) || assigning.is_some() {
                                widgets::learn_ring(ui, t, out.response.rect, waiting.as_ref() == Some(&target));
                            }
                            if learn_on {
                                if out.response.clicked() || out.response.drag_started() {
                                    learn = Some(target.clone());
                                }
                            } else if let Some(i) = assigning.filter(|_| out.response.clicked() || out.response.drag_started()) {
                                // from where it is to its top, as Assign to macro does; the rest
                                // of this drag leaves it there
                                maps_ops.push(("macro.assign", json!({ "index": i, "node": node, "param": key, "min": shown, "max": info.max }), None));
                                assigned = Some((i, i18n::t(info.name).to_string()));
                                ui.data_mut(|d| d.insert_temp(out.response.id.with("given"), true));
                            } else if let Some(v) = out.value.filter(|_| !ui.data(|d| d.get_temp::<bool>(out.response.id.with("given"))).unwrap_or(false)) {
                                slides.push((key.clone(), v));
                            }
                            if out.response.drag_stopped() {
                                ui.data_mut(|d| d.remove::<bool>(out.response.id.with("given")));
                            }
                            close |= out.response.drag_stopped() || out.response.lost_focus();
                            if let (Some(amount), Some(first)) = (out.amount, maps.first()) {
                                maps_ops.push(("mapping.set", json!({ "node": node, "param": key, "source": first.source, "amount": amount }), Some(format!("map {node} {key} {} amount", first.source))));
                            }
                            // a source dropped again: its settings open instead of a second mapping
                            if let Some(source) = out.dropped {
                                let stored = scene.signal_key(&source).unwrap_or_else(|| source.to_string());
                                match maps.iter().position(|m| m.source == stored) {
                                    Some(i) => egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&out.badges[i])),
                                    None => maps_ops.push(("mapping.add", json!({ "node": node, "param": key, "source": source }), None)),
                                }
                            }
                            for (badge, m) in out.badges.iter().zip(&maps) {
                                if let Some(edit) = widgets::mapping_popup(t, badge, &format!("{} → {}", mapping_source(&scene, &m.source), i18n::t(info.name)), m) {
                                    map_ops(&edit, node, m, &mut maps_ops);
                                    close |= edit.done;
                                }
                            }
                            // the macro's badge: its range, or let go
                            if let (Some((i, x)), Some(badge)) = (&driver, out.badges.get(maps.len())) {
                                let popup = egui::Popup::from_toggle_button_response(badge).frame(kit::popover_frame(ui.ctx(), t)).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).width(260.0);
                                popup.show(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(t.gap * 2.0, t.gap * 2.0);
                                    ui.label(RichText::new(format!("{} → {}", names[*i], i18n::t(info.name))).strong().color(t.fg_text));
                                    for (end, value, field) in [("Macro at 0", x.min, "min"), ("Macro at 1", x.max, "max")] {
                                        let range = ParamInfo { name: end, min: info.min, max: info.max, default: value, curve: info.curve, unit: info.unit };
                                        let out = widgets::slider(ui, t, &range, value, &widgets::Marks::default());
                                        if let Some(v) = out.value {
                                            maps_ops.push(("macro.target", json!({ "index": i, "node": node, "param": key, field: info.clamp(v) }), Some(format!("macro range {node} {key} {field}"))));
                                        }
                                        close |= out.response.drag_stopped() || out.response.lost_focus();
                                    }
                                    if kit::action(ui, t, kit::Variant::Ghost, Some(icons::X), &tr!("Unassign from {}", names[*i])).clicked() {
                                        maps_ops.push(("macro.unassign", json!({ "index": i, "node": node, "param": key }), None));
                                        ui.close();
                                    }
                                });
                            }
                            // right-click: a macro, MIDI
                            kit::context_menu(t, &out.response).show(|ui| {
                                let reset = ui.add_enabled(driver.is_none() && shown != info.default, egui::Button::new(format!("{}  {}", icons::ARROW_U_UP_LEFT, tr!("Reset"))));
                                if reset.on_disabled_hover_text(if driver.is_some() { tr!("A macro drives it: unassign it to reset it") } else { "" }).clicked() {
                                    slides.push((key.clone(), info.default));
                                    ui.close();
                                }
                                ui.separator();
                                ui.menu_button(tr!("Assign to macro"), |ui| {
                                    for (i, name) in names.iter().enumerate() {
                                        if ui.selectable_label(driver.as_ref().is_some_and(|d| d.0 == i), format!("{}  {name}", i + 1)).clicked() {
                                            maps_ops.push(("macro.assign", json!({ "index": i, "node": node, "param": key, "min": shown, "max": info.max }), None));
                                            ui.close();
                                        }
                                    }
                                });
                                if let Some((i, _)) = &driver
                                    && ui.button(tr!("Unassign from {}", names[*i])).clicked()
                                {
                                    maps_ops.push(("macro.unassign", json!({ "index": i, "node": node, "param": key }), None));
                                    ui.close();
                                }
                                ui.separator();
                                let label = if waiting.as_ref() == Some(&target) { tr!("Waiting for a controller…") } else { tr!("Learn MIDI") };
                                if ui.button(format!("{}  {label}", icons::CROSSHAIR)).clicked() {
                                    learn = Some(target.clone());
                                    ui.close();
                                }
                                if let Some(b) = binding
                                    && ui.button(format!("{}  {}", icons::X, tr!("Forget {}", b))).clicked()
                                {
                                    forget = Some(target.clone());
                                    ui.close();
                                }
                            });
                        }
                    }
                }
            }
            for (param, info, value) in switches {
                let driver = scene.driver(node, &param.id).map(|(i, x)| (i, x.at(scene.macros[i].value)));
                let value = driver.map_or(value, |(_, v)| [v, 0.0, 0.0, 0.0]);
                let (r, v) = widgets::toggle(ui, t, info, value[0]);
                let target = Target::Param { node: node.to_string(), param: param.id.clone() };
                if learn_on || waiting.as_ref() == Some(&target) {
                    widgets::learn_ring(ui, t, r.rect, waiting.as_ref() == Some(&target));
                }
                if learn_on {
                    if r.clicked() {
                        learn = Some(target.clone());
                    }
                } else if let Some(v) = v {
                    match driver {
                        Some(_) => slides.push((param.id.clone(), v)),
                        None => edits.push((param.id.clone(), [v, 0.0, 0.0, 0.0], false)),
                    }
                }
                if let Some((i, _)) = driver {
                    kit::hint(ui, t, &tr!("{} drives it", names[i]));
                }
                let binding = bound.iter().find(|(k, _)| *k == param.id).map(|(_, b)| b.as_str());
                let reset = driver.is_none() && value[0] != param.default[0];
                if let Some(ask) = param_menu(t, &r, reset, Some((waiting.as_ref() == Some(&target), binding)), Some((&names, driver.map(|d| d.0)))) {
                    asks.push((ask, param.id.clone(), param.default, [info.min, info.max]));
                }
            }
        });
        if shuffle {
            self.randomize(node, shader, set, &scene);
        }
        // one undo step
        if reset {
            let key = format!("reset {node}");
            for (param, _) in shader.controls.iter().filter(|(p, _)| set.get(&p.id).is_some_and(|v| *v != p.default)) {
                self.run_merge("node.set_param", json!({ "node": node, "param": param.id, "value": param.default }), &key);
            }
            self.session.close_merge();
        }
        // what the right-click menus of switches, choices and colors asked for
        for (ask, id, default, [min, max]) in asks {
            let target = Target::Param { node: node.to_string(), param: id.clone() };
            match ask {
                Ask::Reset => edits.push((id, default, false)),
                Ask::Learn => learn = Some(target),
                Ask::Forget => forget = Some(target),
                // the macro takes it through every choice, or from off to on
                Ask::Assign(i) => maps_ops.push(("macro.assign", json!({ "index": i, "node": node, "param": id, "min": min, "max": max }), None)),
                Ask::Unassign(i) => maps_ops.push(("macro.unassign", json!({ "index": i, "node": node, "param": id }), None)),
            }
        }
        for (param, value, merge) in edits {
            let props = json!({ "node": node, "param": param, "value": value });
            if merge {
                self.run_merge("node.set_param", props, &format!("param {node} {param}"));
            } else {
                self.run("node.set_param", props);
            }
        }
        for (key, v) in slides {
            self.set_param(node, &key, v, &format!("param {node} {key}"));
        }
        for (op, mut props, merge) in maps_ops {
            props["scene"] = json!(scene.id);
            match merge {
                Some(key) => self.run_merge(op, props, &key),
                None => self.run(op, props),
            }
        }
        if let Some((i, param)) = assigned {
            self.assigning = None;
            self.say(Tone::Done, tr!("{} drives {} now: turn it", names[i], param));
        }
        if let Some(target) = learn {
            if !self.learn.on {
                self.say(Tone::Action, tr!("Move a knob or hit a pad on your controller (Esc cancels)"));
            }
            self.learn.wait(target);
        }
        if let Some(target) = forget {
            self.run("binding.remove", json!({ "target": target }));
        }
        if close {
            self.session.close_merge();
        }
    }
}

/// What a right-click on a switch, a choice or a color asked for.
enum Ask {
    Reset,
    Learn,
    Forget,
    Assign(usize),
    Unassign(usize),
}

/// The right-click menu of a parameter that isn't a slider, as a slider has it: Reset (when
/// `reset`), then given `macros` (their names, the one driving it) a macro to assign, and given
/// `learn` (waiting for a controller, the one bound) MIDI learn.
fn param_menu(t: &Theme, r: &Response, reset: bool, learn: Option<(bool, Option<&str>)>, macros: Option<(&[String], Option<usize>)>) -> Option<Ask> {
    let mut ask = None;
    // an id of its own: a choice's menu and a color's picker already open from `r`
    kit::context_menu(t, r).id(r.id.with("param menu")).show(|ui| {
        if ui.add_enabled(reset, egui::Button::new(format!("{}  {}", icons::ARROW_U_UP_LEFT, tr!("Reset")))).clicked() {
            ask = Some(Ask::Reset);
            ui.close();
        }
        if let Some((names, driver)) = macros {
            ui.separator();
            ui.menu_button(tr!("Assign to macro"), |ui| {
                for (i, name) in names.iter().enumerate() {
                    if ui.selectable_label(driver == Some(i), format!("{}  {name}", i + 1)).clicked() {
                        ask = Some(Ask::Assign(i));
                        ui.close();
                    }
                }
            });
            if let Some(i) = driver
                && ui.button(tr!("Unassign from {}", names[i])).clicked()
            {
                ask = Some(Ask::Unassign(i));
                ui.close();
            }
        }
        if let Some((waiting, binding)) = learn {
            ui.separator();
            let label = if waiting { tr!("Waiting for a controller…") } else { tr!("Learn MIDI") };
            if ui.button(format!("{}  {label}", icons::CROSSHAIR)).clicked() {
                ask = Some(Ask::Learn);
                ui.close();
            }
            if let Some(b) = binding
                && ui.button(format!("{}  {}", icons::X, tr!("Forget {}", b))).clicked()
            {
                ask = Some(Ask::Forget);
                ui.close();
            }
        }
    });
    ask
}

/// A macro's knob: 0..1, shown in percent.
const MACRO: ParamInfo = ParamInfo { name: "Macro", min: 0.0, max: 1.0, default: 0.0, curve: Curve::Linear, unit: "%" };
/// How long a Shift+click on a variation morphs, in beats: a bar.
const MORPH_BEATS: f32 = 4.0;
/// A macro's number over its knob (widgets::DECK_KNOB_H), what the deck is built around.
const MACROS_H: f32 = 76.0;
/// A deck block's title row, and how wide the variations' block is (four keys a row).
const DECK_HEAD: f32 = 20.0;
const VARIATIONS_W: f32 = 132.0;
/// The narrowest a macro's slot in the deck gets: its knob and a gap.
const MACRO_SLOT: f32 = 64.0;

/// A variation's key: a quarter of its block's row.
fn variation_key(t: &Theme) -> egui::Vec2 {
    egui::vec2((VARIATIONS_W - 3.0 * t.gap) / 4.0, 26.0)
}

/// An Audio In's threshold sliders' top (dB): the top of its graphs in Simple mode.
const RISE_TOP_DB: f32 = 30.0;

/// An Audio In's hit as its detector hears it: its name (with the model's badge when the model
/// decides), then its signal, threshold and hits over the last seconds, and whether the gate holds
/// them back; the threshold its line was dragged to.
fn hit_graph(ui: &mut egui::Ui, t: &Theme, hit: usize, graph: &std::collections::VecDeque<(f64, f32, bool)>, detection: &velo_audio::Settings, gated: bool, now: f64) -> Option<f32> {
    let model = detection.mode == velo_audio::Mode::Model;
    ui.horizontal(|ui| {
        ui.label(RichText::new(source_label(velo_audio::HITS[hit])).font(t.body()).color(t.fg_text));
        if model {
            widgets::model_badge(ui, t);
        }
    });
    let tip = if model { tr!("The drum model's probability, and the one it fires at: drag the line to move it") } else { tr!("How much the band rose (dB), and the threshold: a hit where it crosses the line, drag it to move it") };
    let (r, moved) = widgets::detector_graph(ui, t, graph, detection.thresholds()[hit], if model { 1.0 } else { RISE_TOP_DB }, now, live::GRAPH_S);
    if gated {
        ui.painter().text(r.rect.left_top() + egui::vec2(8.0, 6.0), egui::Align2::LEFT_TOP, tr!("Under the gate: no hits"), egui::FontId::proportional(t.text_xs), t.fg_muted);
    }
    r.on_hover_text(tip);
    ui.add_space(t.gap);
    moved
}

/// A source's name as the interface says it ("kick" → "Kick").
pub(crate) fn source_label(id: &str) -> &'static str {
    match id {
        "level" => tr!("Level"),
        "bass" => tr!("Bass"),
        "mid" => tr!("Mid"),
        "high" => tr!("High"),
        "kick" => tr!("Kick"),
        "snare" => tr!("Snare"),
        "hat" => tr!("Hat"),
        "beat" => tr!("Beat"),
        "bar_phase" | "bar" => tr!("Bar"),
        "melody" => tr!("Melody"),
        _ => tr!("Unknown source"),
    }
}

/// What a mapping's popover changed, as operations on it (a drag merged into one step).
fn map_ops(edit: &widgets::MapEdit, node: &str, m: &Mapping, ops: &mut Vec<(&'static str, Value, Option<String>)>) {
    let props = json!({ "node": node, "param": m.param, "source": m.source });
    let merge = |what: &str| Some(format!("map {node} {} {} {what}", m.param, m.source));
    let mut set = |field: &str, value: Value, merge: Option<String>| {
        let mut p = props.clone();
        p[field] = value;
        ops.push(("mapping.set", p, merge));
    };
    if let Some(a) = edit.amount {
        set("amount", json!(a), merge("amount"));
    }
    if let Some(ms) = edit.attack_ms {
        set("attack_ms", json!(ms), merge("attack"));
    }
    if let Some(ms) = edit.release_ms {
        set("release_ms", json!(ms), merge("release"));
    }
    if let Some(on) = edit.invert {
        set("invert", json!(on), None);
    }
    if edit.remove {
        ops.push(("mapping.remove", props, None));
    }
}

/// A parameter as its inspector names it, by a mapping's key ("zoom", a vec2's "offset.y").
/// A file in use: its icon and its name, the whole path on hover.
fn file_row(ui: &mut egui::Ui, t: &Theme, icon: &str, path: &str) {
    let (row, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.control_h), egui::Sense::hover());
    ui.painter().rect_filled(row, t.radius_sm, t.bg_field);
    kit::icon_at(ui.painter(), egui::pos2(row.left() + t.pad + 7.0, row.center().y), icon, 15.0, t.accent);
    let name = std::path::Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
    kit::fit_text(ui.painter(), egui::pos2(row.left() + t.pad + 22.0, row.center().y), egui::Align2::LEFT_CENTER, name, t.body(), t.fg_text, row.width() - 2.0 * t.pad - 22.0);
    resp.on_hover_text(path);
}

/// What a node is, in a word: a generator, an effect, a signal…
pub fn role_of(k: &velo_core::NodeKind) -> &'static str {
    match k.category {
        Category::Generator => "Generator",
        Category::Effect => "Effect",
        Category::Signal if k.group() == "Logic" => "Logic",
        Category::Signal => "Signal",
        c => c.label(),
    }
}

fn param_name(node: &velo_core::Node, key: &str) -> String {
    let (param, axis) = key.split_once('.').map_or((key, None), |(p, a)| (p, Some(a)));
    let info = live::shader(&node.kind, node.code.as_deref()).and_then(|s| s.controls.iter().find(|(p, _)| p.id == param)).and_then(|(_, infos)| infos.get(usize::from(axis == Some("y"))));
    info.map_or_else(|| key.to_string(), |i| i18n::t(i.name).to_string())
}

/// A mapping's source by name: a shared one's, or a signal node's ("Audio In · Kick").
fn mapping_source(scene: &velo_core::Scene, source: &str) -> String {
    let Some((id, port)) = velo_core::signal_output(source) else { return source_label(source).to_string() };
    let Some(n) = scene.node(id) else { return tr!("Unknown source").to_string() };
    match node_kind(&n.kind).map(|k| k.outputs).filter(|o| o.len() > 1) {
        Some(outputs) => format!("{} · {}", n.name, outputs.get(usize::from(port)).map_or("?", |o| source_label(o))),
        None => n.name.clone(),
    }
}

impl App {
    /// The whole window, one frame: everything `ui()` does but the measuring (also what the
    /// headless interaction tests run).
    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.info = std::mem::take(&mut self.info_next);
        self.handle_keys(&ctx);
        self.midi_frame(ctx.input(|i| i.time));
        self.follow_music();
        // pictures dropped on the AI panel go with its next message; ISF shaders dropped on the
        // window come in as Shader nodes
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        let dropped = self.chat_take_drop(&ctx, dropped);
        for path in dropped.iter().filter(|p| p.extension().is_some_and(|e| ["fs", "frag", "glsl"].iter().any(|x| e.eq_ignore_ascii_case(x)))) {
            self.import_isf(path);
        }
        // pictures, GIFs and folders of frames as Image nodes; several files at once, an array
        let media: Vec<PathBuf> = dropped.iter().filter(|p| p.is_dir() || p.extension().is_some_and(|e| velo_engine::media::EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))).cloned().collect();
        let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = media.into_iter().partition(|p| p.is_dir());
        if files.len() > 1 {
            self.import_media_set(&files);
        }
        for path in folders.iter().chain(files.iter().filter(|_| files.len() == 1)) {
            self.import_media(path);
        }
        // words: a Text List of each file's lines or rows
        for path in dropped.iter().filter(|p| p.extension().is_some_and(|e| velo_core::TEXT_FILES.iter().any(|x| e.eq_ignore_ascii_case(x)))) {
            self.import_text_list(path);
        }
        self.sync_live(&ctx);

        // The bars touch the window's edges with 1-px dividers. Between them, Figma UI3's
        // layout: the canvas fills the window and the panels float on it as rounded cards.
        let t = self.theme.clone();
        let fill = |c| Frame::new().fill(c);
        egui::Panel::top("top").frame(fill(t.bg_panel)).exact_size(t.bar_h).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").frame(fill(t.bg_panel)).exact_size(24.0).show(ui, |ui| self.status_bar(ui));
        let full = ui.available_rect_before_wrap();
        ui.painter().rect_filled(full, 0, t.bg_base);
        // the picture as the canvas' backdrop, whole (letterboxed), under the grid and the nodes
        // (alone while the nodes are hidden)
        if (self.settings.preview_background || self.canvas.hidden)
            && self.settings.split == Split::Off
            && let Some(id) = self.live.as_ref().and_then(live::Live::picture)
        {
            let area = self.canvas.view.unwrap_or(full);
            let c = self.session.project.canvas;
            let aspect = c.width.max(1) as f32 / c.height.max(1) as f32;
            let size = if area.width() / area.height().max(1.0) > aspect { egui::vec2(area.height() * aspect, area.height()) } else { egui::vec2(area.width(), area.width() / aspect) };
            egui::Image::new((id, size)).paint_at(ui, egui::Rect::from_center_size(area.center(), size));
        }
        // split: the nodes keep the canvas but the picture's side (as the center was last frame)
        let nodes = match (self.split_panes(), self.settings.split) {
            (Some((_, picture)), Split::Right) => egui::Rect::from_min_max(full.min, egui::pos2(picture.left(), full.bottom())),
            (Some((_, picture)), Split::Bottom) => egui::Rect::from_min_max(full.min, egui::pos2(full.right(), picture.top())),
            _ => full,
        };
        if self.session.active().is_some() {
            self.node_canvas(&mut ui.new_child(egui::UiBuilder::new().max_rect(nodes)));
        }
        // the panels slide in and out (the top bar's toggles, Cmd+B, Cmd+Alt+B, Cmd+J, Cmd+\
        // for all of them); dragging an edge past its minimum closes one too. They're docked,
        // edge to edge: egui's 1-px separator is the one divider between a panel and the canvas.
        // the deck before the side panels: under everything, the window's width
        let (before, mut open) = (self.settings.panels, self.settings.panels);
        egui::Panel::bottom("deck").frame(fill(t.bg_panel)).exact_size(DECK_HEAD + 2.0 * t.gap + MACROS_H + t.text_xs + 4.0 + 2.0 * t.pad).show_collapsible(ui, &mut open.sources, |ui| kit::docked(ui, |ui| self.deck(ui)));
        egui::Panel::left("scenes").frame(fill(t.bg_panel)).resizable(true).default_size(228.0).size_range(188.0..=368.0).show_collapsible(ui, &mut open.scenes, |ui| kit::docked(ui, |ui| self.scenes_rail(ui)));
        // the AI outermost on the right, beside the inspector
        egui::Panel::right("chat").frame(fill(t.bg_panel)).resizable(true).default_size(340.0).size_range(280.0..=560.0).show_collapsible(ui, &mut open.chat, |ui| kit::docked(ui, |ui| self.chat_panel(ui)));
        // docked on the right, unless it floats or the split canvas holds it beside the nodes
        let mut docked = open.inspector && !self.settings.inspector_floating && self.settings.split == Split::Off;
        egui::Panel::right("inspector").frame(fill(t.bg_panel)).resizable(true).default_size(268.0).size_range(228.0..=428.0).show_collapsible(ui, &mut docked, |ui| kit::docked(ui, |ui| self.inspector(ui)));
        if !self.settings.inspector_floating && self.settings.split == Split::Off {
            open.inspector = docked;
        }
        // a panel can close itself while it draws (its chevron): that wins over what egui saw
        let now = self.settings.panels;
        let pick = |b: bool, n: bool, o: bool| if n != b { n } else { o };
        self.settings.panels = Panels {
            scenes: pick(before.scenes, now.scenes, open.scenes),
            inspector: pick(before.inspector, now.inspector, open.inspector),
            sources: pick(before.sources, now.sources, open.sources),
            chat: pick(before.chat, now.chat, open.chat),
        };
        egui::CentralPanel::default().frame(Frame::NONE).show(ui, |ui| self.center(ui));
        // the inspector in a window of its own: drag it by its bar, size it by its edges; the
        // margin keeps its square bar inside the window's rounded corners
        if self.settings.inspector_floating && self.settings.panels.inspector {
            let frame = Frame::new().fill(t.bg_panel).stroke(egui::Stroke::new(1.0, t.line)).corner_radius(t.radius_md).inner_margin(3).shadow(ctx.global_style().visuals.window_shadow);
            // first under the preview card, at the right
            let at = self.center_rect.map_or(egui::pos2(900.0, 80.0), |r| r.right_top() + egui::vec2(-300.0 - t.pad, 240.0));
            egui::Window::new("inspector").title_bar(false).frame(frame).default_pos(at).default_size([300.0, 520.0]).min_size([240.0, 200.0]).resizable(true).constrain(true).show(&ctx, |ui| self.inspector(ui));
        }
        self.start_ui(&ctx);
        self.node_menu(&ctx);
        self.preferences(&ctx);
        self.code_editor(&ctx);
        self.shortcut_sheet(&ctx);
        self.rename_dialog(&ctx);
        self.palette_ui(&ctx);
        self.file_modals(&ctx);
        self.toast_ui(&ctx);
        self.splash_ui(&ctx);
        // learn mode says what it waits for, over whatever the mouse is on
        if let Some(target) = &self.learn.target {
            self.info_next = tr!("Waiting for {}: move a knob or hit a pad on your controller · Esc cancels", self.target_name(target));
        } else if self.learn.on {
            self.info_next = tr!("MIDI learn: click a control, then move a knob or hit a pad · L or Esc leaves").into();
        }
        if let Some(board) = &mut self.specimen
            && !board.show(&ctx, &t, self.cpu.stats(), self.cpu.layout())
        {
            self.specimen = None;
        }
    }
}

#[cfg(test)]
mod pilot_tests {
    use super::{Layer, Pilot, Scene, next_turn};

    #[test]
    fn the_autopilot_takes_turns_among_the_scenes_left_in() {
        let scene = |id: &str, layer: Layer, manual: bool| Scene { id: id.into(), layer, manual, ..Default::default() };
        let scenes = [scene("a", Layer::Scene, false), scene("t", Layer::Top, false), scene("b", Layer::Scene, false), scene("m", Layer::Scene, true), scene("c", Layer::Scene, false)];
        assert_eq!(next_turn(&scenes, Some("a"), None).as_deref(), Some("b"), "past the layer");
        assert_eq!(next_turn(&scenes, Some("c"), None).as_deref(), Some("a"), "past the one left out, round to the first");
        assert_eq!(next_turn(&scenes, Some("m"), None).as_deref(), Some("a"), "from a scene out of the turns: the first");
        for r in [0.0, 0.3, 0.6, 0.99] {
            let next = next_turn(&scenes, Some("b"), Some(r)).unwrap();
            assert!(next != "b" && ["a", "c"].contains(&next.as_str()), "{r}: {next}");
        }
        assert_eq!(next_turn(&scenes[..2], Some("a"), None), None, "one scene and a layer: nothing to turn to");
    }

    const BPM: f64 = 120.0;

    /// The bass of four-on-the-floor at 120 BPM, `gain` at its loudest: full on each kick,
    /// falling to a tenth before the next.
    fn kicks(seconds: f64, gain: f32) -> f32 {
        gain * (0.1 + 0.9 * (-5.0 * (seconds * BPM / 60.0).fract() as f32).exp())
    }

    /// When (in seconds) a pilot fresh at `from` drops, hearing `bass` (a function of the time
    /// in seconds) at 60 frames a second until `to`.
    fn drops(p: &mut Pilot, from: f64, to: f64, bass: impl Fn(f64) -> f32) -> Vec<f64> {
        let frames = (from * 60.0) as usize..(to * 60.0) as usize;
        frames.map(|i| i as f64 / 60.0).filter(|&s| p.dropped(bass(s), 1.0 / 60.0, s * BPM / 60.0)).collect()
    }

    #[test]
    fn steady_kicks_never_drop() {
        for gain in [1.0, 0.25] {
            let mut p = Pilot::default();
            // the first bars are the bass coming in from nothing (the 4 bars after beat 0 never drop)
            assert_eq!(drops(&mut p, 0.0, 8.0, |s| kicks(s, gain)), [0.0; 0]);
            assert_eq!(drops(&mut p, 8.0, 240.0, |s| kicks(s, gain)), [0.0; 0], "gain {gain}");
            assert!(p.fast - p.slow <= 0.0, "the fast bass stays under the slow one: {} {}", p.fast, p.slow);
        }
    }

    #[test]
    fn the_bass_back_after_a_breakdown_drops_once() {
        // 16 s without the kick (pads, a twelfth of its bass), whatever the input gain
        for gain in [1.0, 0.25] {
            let music = |s: f64| if (30.0..46.0).contains(&s) { gain * 0.08 } else { kicks(s, gain) };
            let mut p = Pilot::default();
            assert_eq!(drops(&mut p, 0.0, 46.0, music), [0.0; 0], "none before or in the breakdown (gain {gain})");
            let back = drops(&mut p, 46.0, 120.0, music);
            assert_eq!(back.len(), 1, "one drop when the bass returns (gain {gain}): {back:?}");
            assert!((46.0..48.0).contains(&back[0]), "within its first bar: {back:?}");
        }
        // a bar without the kick is a fill, not a breakdown
        let fill = |s: f64| if (30.0..32.0).contains(&s) { 0.08 } else { kicks(s, 1.0) };
        assert_eq!(drops(&mut Pilot::default(), 0.0, 60.0, fill), [0.0; 0]);
    }

    #[test]
    fn quiet_input_never_drops() {
        // hiss with the same shape: kicks, a breakdown, kicks again, all under the quiet floor
        let hiss = |s: f64| if (30.0..46.0).contains(&s) { 0.0 } else { kicks(s, 0.04) };
        assert_eq!(drops(&mut Pilot::default(), 0.0, 120.0, hiss), [0.0; 0]);
        assert_eq!(drops(&mut Pilot::default(), 0.0, 120.0, |_| 0.0), [0.0; 0], "silence");
    }

    #[test]
    fn two_drops_are_4_bars_apart() {
        // the bass in from nothing at beat 20: a drop
        let mut p = Pilot::default();
        let first = drops(&mut p, 10.0, 12.0, |_| 1.0);
        assert_eq!(first, [10.0]);
        // from nothing again 2 bars later: it would be one, were the last not so near
        (p.fast, p.slow) = (0.0, 0.0);
        assert_eq!(drops(&mut p, 14.0, 16.0, |_| 1.0), [0.0; 0]);
        // and past 4 bars (8 s) it is, once it's well over how strong the bass has been
        (p.fast, p.slow) = (0.0, 0.0);
        let again = drops(&mut p, 18.5, 20.0, |_| 1.0);
        assert!(again.len() == 1 && again[0] < 19.0, "{again:?}");
    }

    /// The same through the analyzer the app listens with: the test loop for a minute, its
    /// low end filtered out from 24 s to 40 s as a DJ's filter does (the level stays, the
    /// bass goes).
    #[test]
    fn the_analyzers_bass_drops_once_after_a_filtered_breakdown() {
        use velo_audio::{Analyzer, HOP, Tracker, synth};
        const RATE: f32 = 48_000.0;
        let mut samples = synth::techno(RATE, BPM as f32, 60.0, 7).samples;
        // two one-pole high-passes at 800 Hz: the kick and the bass line 30 dB down
        let k = 1.0 - (-std::f32::consts::TAU * 800.0 / RATE).exp();
        let mut low = [0.0f32; 2];
        for s in &mut samples[(24.0 * RATE) as usize..(40.0 * RATE) as usize] {
            for l in &mut low {
                *l += (*s - *l) * k;
                *s -= *l;
            }
        }
        let (mut analyzer, mut tracker, mut p) = (Analyzer::new(RATE), Tracker::new(), Pilot::default());
        let dt = HOP as f32 / RATE;
        let (mut hops, mut drops, mut levels) = (0usize, Vec::new(), [Vec::new(), Vec::new()]);
        analyzer.push(&samples, |hop| {
            tracker.hop(hop, dt);
            let (f, seconds) = (tracker.features(), hops as f64 * f64::from(dt));
            if p.dropped(f.bass, dt, seconds * BPM / 60.0) {
                drops.push(seconds);
            }
            // the level and the bass over the last 8 s of the breakdown
            if (32.0..40.0).contains(&seconds) {
                levels[0].push(f.level);
                levels[1].push(f.bass);
            }
            hops += 1;
        });
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean(&levels[0]) > 3.0 * mean(&levels[1]), "the breakdown keeps its level, not its bass: {} {}", mean(&levels[0]), mean(&levels[1]));
        assert_eq!(drops.len(), 1, "{drops:?}");
        assert!((40.0..42.0).contains(&drops[0]), "when the bass returns: {drops:?}");
    }
}

#[cfg(test)]
impl App {
    /// An app with no window and nothing on disk, motion off: for the headless UI tests.
    pub fn for_tests() -> Self {
        let settings = Settings { reduce_motion: true, ..Default::default() };
        Self {
            restarted: None,
            pilot: Pilot::default(),
            session: Session::default(),
            theme: settings.effective_theme(),
            settings,
            canvas: canvas::State::default(),
            center_rect: None,
            palette: None,
            show_settings: false,
            show_help: false,
            rename: None,
            editing: None,
            watch: files::Watch::default(),
            agent: agent::Agent::default(),
            chat: chat::Chat::default(),
            osc: None,
            auto: shell::Auto::default(),
            frame_times: std::collections::VecDeque::new(),
            info: String::new(),
            info_next: String::new(),
            toast: None,
            updates: update::Updates::default(),
            specimen: None,
            cpu: Cpu::default(),
            usage: usage::Usage::new(),
            splash: None,
            splash_from: None,
            live: None,
            midi: midi::Midi::default(),
            learn: midi::Learn::default(),
            assigning: None,
            space: None,
            demo_drop: None,
            start: false,
            painted: Instant::now(),
            waited: 0.0,
            file: None,
            saved: Project::default(),
            autosaved: Project::default(),
            last_autosave: 0.0,
            pending: None,
            quitting: None,
            recovery_later: false,
            recovery: None,
        }
    }
}

impl eframe::App for App {
    /// Housekeeping that must also run while the window is minimized.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        self.autosave(now);
        self.watch_shaders(now);
        self.poll_agent(ctx, now);
        self.poll_osc(ctx, now);
        self.chat_poll();
        self.poll_updates(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        // never faster than a display shows: a window macOS isn't showing (covered, on another
        // Space) gets no vsync, and anything that asks for a repaint every frame (a card beating
        // with the music) would spin a core. 120 a second in front, 60 behind another app.
        let least = Duration::from_micros(if ctx.input(|i| i.focused) { 8_000 } else { 16_000 });
        let waited = std::mem::take(&mut self.waited);
        if std::env::var_os("VELO_CAPTURE_TO").is_none()
            && let Some(wait) = least.checked_sub(self.painted.elapsed())
        {
            let before = Instant::now();
            std::thread::sleep(wait);
            self.waited = before.elapsed().as_secs_f32();
        }
        self.painted = Instant::now();
        let started = Instant::now();
        // the frame before this one, without what it waited
        if let Some(cpu) = frame.info().cpu_usage {
            self.cpu.push(now, (cpu - waited).max(0.0), self.cpu.last_layout);
        }
        if std::env::var_os("VELO_CAPTURE_TO").is_some() && now > 4.8 && !self.cpu.reported {
            self.cpu.reported = true;
            let ((avg, worst), (layout, layout_worst)) = (self.cpu.stats(), self.cpu.layout());
            eprintln!("velo: UI frame {:.2} ms average, {:.2} ms worst; our ui() {:.2} ms average, {:.2} ms worst; {} frames (last 2 s)", avg * 1000.0, worst * 1000.0, layout * 1000.0, layout_worst * 1000.0, self.cpu.samples.len());
            if let Some(live) = &self.live {
                let s = &live.engine.status;
                let output = self.live_output().map_or("none".to_string(), |o| format!("{o:?}"));
                eprintln!("velo: engine {:.1} fps, last frame {} µs on the render thread, {} frames, {} late; output {output}", live.fps, s.frame_us.load(Relaxed), s.frames.load(Relaxed), s.late.load(Relaxed));
                if let Some(scene) = self.session.active() {
                    let costs: Vec<String> = scene.nodes.iter().filter_map(|n| live.cost(&n.id).map(|c| format!("{} {c:.2}", n.kind))).collect();
                    eprintln!("velo: GPU ms per node of {}: {}", scene.name, costs.join(", "));
                }
            }
        }
        // VELO_DEMO_PERF: the engine's rate on stderr every 2 s, in a normal run (measuring)
        if std::env::var_os("VELO_DEMO_PERF").is_some()
            && let Some(live) = &self.live
        {
            static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            if LAST.swap(now as u64 / 2, Relaxed) != now as u64 / 2 {
                let s = &live.engine.status;
                let (avg, worst) = self.cpu.stats();
                eprintln!("velo: t {now:.0} s, engine {:.1} fps, frame {} µs, {} late, {} stalls; UI {:.2} ms average, {:.2} ms worst, {} frames", live.fps, s.frame_us.load(Relaxed), s.late.load(Relaxed), s.stalls.load(Relaxed), avg * 1000.0, worst * 1000.0, self.cpu.samples.len());
            }
        }
        // VELO_DEMO_TOAST=done|error|action: a toast fully out when the 5-s capture is taken
        if now > 4.0
            && let Ok(tone) = std::env::var("VELO_DEMO_TOAST")
        {
            static SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if !SHOWN.swap(true, Relaxed) {
                match tone.as_str() {
                    "error" => self.say(Tone::Error, tr!("A node couldn't be built: {}", "line 12: no identifier `tim` (did you mean `time`?)")),
                    "action" => self.say(Tone::Action, tr!("Move a knob or hit a pad on your controller (Esc cancels)")),
                    _ => self.say(Tone::Done, tr!("Saved {}", "Friday set.velo")),
                }
            }
        }
        files::capture_to_file(&ctx);
        self.draw(ui);
        // live: at the preview's rate (it comes at most every 16 ms); else a frame a second at
        // rest, for the autosave check
        ctx.request_repaint_after(if self.live.is_some() { Duration::from_millis(16) } else { Duration::from_secs(1) });
        self.cpu.last_layout = started.elapsed().as_secs_f32();
    }

    /// Screenshot runs take no input: macOS still focuses the off-screen capture window, and
    /// keys, scrolls and pinches meant for other apps would edit or move what's captured.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if std::env::var_os("VELO_CAPTURE_TO").is_some() {
            raw_input.events.clear();
            let at = |var: &str| std::env::var(var).ok().and_then(|v| v.split_once(',').and_then(|(x, y)| Some(egui::pos2(x.parse().ok()?, y.parse().ok()?))));
            // VELO_DEMO_POINTER=x,y: the pointer held there, to capture hover states
            if let Some(egui::Pos2 { x, y }) = at("VELO_DEMO_POINTER") {
                // VELO_DEMO_DRAG=x,y: the button pressed there 3 s in and held, so the pointer drags
                // from there (a wire mid-drag); VELO_DEMO_DROP=seconds: let go then
                static PRESSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                static DROPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                let time = raw_input.time.unwrap_or(0.0);
                match at("VELO_DEMO_DRAG") {
                    Some(from) if time > 3.0 && !PRESSED.swap(true, Relaxed) => {
                        raw_input.events.push(Event::PointerMoved(from));
                        raw_input.events.push(Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() });
                    }
                    _ => raw_input.events.push(Event::PointerMoved(egui::pos2(x, y))),
                }
                if PRESSED.load(Relaxed) && std::env::var("VELO_DEMO_DROP").ok().and_then(|s| s.parse::<f64>().ok()).is_some_and(|s| time > s) && !DROPPED.swap(true, Relaxed) {
                    raw_input.events.push(Event::PointerButton { pos: egui::pos2(x, y), button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() });
                }
                // VELO_DEMO_CLICK: and one click there 3 s in (an open menu)
                static CLICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                // (VELO_DEMO_CLICK=right: the right button)
                if let Some(which) = std::env::var_os("VELO_DEMO_CLICK")
                    && raw_input.time.unwrap_or(0.0) > 3.0
                    && !CLICKED.swap(true, Relaxed)
                {
                    let button = if which == "right" { egui::PointerButton::Secondary } else { egui::PointerButton::Primary };
                    for pressed in [true, false] {
                        raw_input.events.push(Event::PointerButton { pos: egui::pos2(x, y), button, pressed, modifiers: Default::default() });
                    }
                }
            }
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
    }

    fn on_exit(&mut self) {
        self.exit_cleanup();
        self.finish_update();
    }
}

/// What the UI costs per frame over the last two seconds: eframe's whole frame (our ui(),
/// tessellation, painting; its docs say without the vsync wait) and our ui() alone.
#[derive(Default)]
struct Cpu {
    /// (when, frame, ui()) in seconds.
    samples: std::collections::VecDeque<(f64, f32, f32)>,
    last_layout: f32,
    /// `VELO_CAPTURE_TO` runs print it once.
    reported: bool,
}

impl Cpu {
    fn push(&mut self, now: f64, frame: f32, layout: f32) {
        self.samples.push_back((now, frame, layout));
        while self.samples.front().is_some_and(|s| now - s.0 > 2.0) {
            self.samples.pop_front();
        }
    }

    fn summary(&self, pick: impl Fn(&(f64, f32, f32)) -> f32) -> (f32, f32) {
        let n = self.samples.len().max(1) as f32;
        (self.samples.iter().map(&pick).sum::<f32>() / n, self.samples.iter().map(&pick).fold(0.0, f32::max))
    }

    /// Average and worst whole frame, in seconds.
    fn stats(&self) -> (f32, f32) {
        self.summary(|s| s.1)
    }

    /// Average and worst of our ui() alone.
    fn layout(&self) -> (f32, f32) {
        self.summary(|s| s.2)
    }
}
