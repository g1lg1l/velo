//! Our event loop (spike S1): eframe runs as an `ApplicationHandler` inside it
//! (`eframe::create_native`), so the output windows are ours: one per display the picture goes
//! to, and a plain window. The main thread creates output
//! windows and their surfaces; the render thread configures and presents them, and the main
//! thread never waits for it. Fullscreen is a stage window (borderless, covering the display,
//! above other windows), not the OS fullscreen, which on macOS is a Space with an animation.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use eframe::{UserEvent, wgpu};
use velo_engine::projection::{self, Projection};
use velo_engine::{Command, Engine};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowId, WindowLevel};

/// Where the output goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A plain window, for trying things out on one screen.
    Window,
    /// A stage window covering the display with this name.
    Display(String),
}

/// A display, as the output menu lists it.
#[derive(Clone, Debug)]
pub struct Display {
    pub name: String,
    /// In pixels.
    pub size: [u32; 2],
    pub hz: Option<f32>,
}

/// UI → event loop: windows can only be made there.
pub enum Request {
    /// An output there too (one already there is picked for the projection's settings).
    Open(Target),
    /// That output closed, or every one (None).
    Close(Option<Target>),
    /// List the displays again (the output menu opened).
    Displays,
    /// Start the engine again (the GPU was lost, or the render thread stopped drawing): the
    /// outputs reopen where they were.
    Restart,
}

/// What the UI and the event loop share. Both live on the main thread.
#[derive(Default)]
pub struct Shared {
    pub requests: Vec<Request>,
    pub displays: Vec<Display>,
    /// Where the outputs are now, in the order they opened: set by the event loop once each
    /// window exists.
    pub outputs: Vec<Target>,
    /// The output whose projection the Output menu sets and adjusting moves (the last opened
    /// unless another is picked): see [`Shared::current`].
    pub picked: Option<Target>,
    /// A restarted engine, for the UI to take up (or why it didn't start).
    pub fresh: Option<Result<Rc<Engine>, String>>,
    /// A display an output was on, unplugged: that output closed, for the UI to say.
    pub unplugged: Option<String>,
    /// Each output's corner pin and masks, by where it goes ([`key`]): the UI keeps them with
    /// the preferences, the event loop sends the one in use to the engine.
    pub projections: std::collections::BTreeMap<String, Projection>,
    /// Adjusting the projection: handles on the output, moved with the mouse and keys there.
    pub adjust: Option<Adjust>,
}

impl Shared {
    /// The output whose projection is set up: the one picked while it's open, else the last opened.
    pub fn current(&self) -> Option<&Target> {
        self.picked.as_ref().filter(|p| self.outputs.contains(p)).or(self.outputs.last())
    }

    /// The current output's projection, made if it has none.
    pub fn projection(&mut self) -> Option<&mut Projection> {
        let key = key(self.current()?);
        Some(self.projections.entry(key).or_default())
    }

    /// Stops adjusting, or drawing a mask: one left with under three points goes.
    pub fn finish(&mut self, stop: bool) {
        if let Some(p) = self.projection() {
            p.masks.retain(|m| m.len() >= 3);
        }
        if stop {
            self.adjust = None;
        } else if let Some(a) = &mut self.adjust {
            (a.drawing, a.selected) = (false, None);
        }
    }

    /// Starts a new mask: clicks on the output add its points.
    pub fn draw_mask(&mut self) {
        self.finish(false);
        if let Some(p) = self.projection().filter(|p| p.masks.len() < projection::MAX_MASKS) {
            p.masks.push(Vec::new());
            self.adjust = Some(Adjust { drawing: true, ..Default::default() });
        }
    }
}

/// What a projection is kept under: the display's name ("" for a window).
pub fn key(target: &Target) -> String {
    match target {
        Target::Window => String::new(),
        Target::Display(name) => name.clone(),
    }
}

/// Adjusting the projection, on the output window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Adjust {
    /// The handle picked ([`Projection::handles`]' order): the arrow keys move it.
    pub selected: Option<usize>,
    /// Clicks add points to the last mask.
    pub drawing: bool,
    /// The mouse holds the picked handle.
    pub dragging: bool,
}

/// An open output: where it goes, its window and the engine's id for it.
struct Open {
    target: Target,
    window: Arc<Window>,
    id: u64,
    /// The projection the engine was last sent for it, with its handles (None: nothing yet).
    sent: Option<(Projection, Option<Option<usize>>)>,
}

pub struct Host<'a> {
    pub eframe: eframe::EframeWinitApplication<'a>,
    pub engine: Option<Rc<Engine>>,
    pub shared: Rc<RefCell<Shared>>,
    outputs: Vec<Open>,
    /// Closed outputs by id, dropped here on the main thread once the render thread let go of them.
    closing: Vec<(u64, Arc<Window>)>,
    /// The next output's id.
    next: u64,
    /// When the outputs' displays were last looked for.
    looked: std::time::Instant,
    /// The mouse on an output window, in pixels, and Shift held there.
    cursor: [f32; 2],
    shift: bool,
}

impl<'a> Host<'a> {
    pub fn new(eframe: eframe::EframeWinitApplication<'a>, engine: Option<Rc<Engine>>, shared: Rc<RefCell<Shared>>) -> Self {
        Self { eframe, engine, shared, outputs: Vec::new(), closing: Vec::new(), next: 1, looked: std::time::Instant::now(), cursor: [0.0; 2], shift: false }
    }

    /// The mouse and keys on the adjusted output's window while its projection is adjusted: drag
    /// a handle, click to add a mask's point, arrows nudge the picked handle a pixel (Shift:
    /// ten). True when the event was theirs.
    fn adjust(&mut self, id: WindowId, event: &WindowEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let mut shared = self.shared.borrow_mut();
        let Some(mut adjust) = shared.adjust else { return false };
        // only the output being adjusted takes the mouse and keys
        let Some(window) = shared.current().and_then(|c| self.outputs.iter().find(|o| &o.target == c)).map(|o| &o.window).filter(|w| w.id() == id) else { return false };
        let size = window.inner_size();
        let [w, h] = [size.width.max(1) as f32, size.height.max(1) as f32];
        let at = [self.cursor[0] / w, self.cursor[1] / h];
        // as far as a handle reaches (the warp shader draws them 10 px across at 1080), and some
        let reach = (16.0 * h / 1080.0).max(8.0);
        let Some(p) = shared.projection() else { return false };
        let mut finish = None;
        match event {
            WindowEvent::CursorMoved { .. } => {
                if let (true, Some(q)) = (adjust.dragging, adjust.selected.and_then(|i| p.handle_mut(i))) {
                    *q = at.map(|v| v.clamp(0.0, 1.0));
                }
            }
            WindowEvent::MouseInput { state, button: winit::event::MouseButton::Left, .. } => {
                adjust.dragging = false;
                if state.is_pressed() {
                    let near = p.handles().enumerate().map(|(i, q)| (i, ((q[0] - at[0]) * w).hypot((q[1] - at[1]) * h))).filter(|n| n.1 < reach).min_by(|a, b| a.1.total_cmp(&b.1)).map(|n| n.0);
                    let last = p.masks.len().checked_sub(1);
                    match near {
                        // the mask being drawn closed on its first point
                        Some(i) if adjust.drawing && last.map(|m| p.first_of(m)) == Some(i) => finish = Some(false),
                        Some(i) => (adjust.selected, adjust.dragging) = (Some(i), true),
                        None if adjust.drawing => {
                            if p.handles().count() < projection::MAX_POINTS + 4
                                && let Some(m) = p.masks.last_mut()
                            {
                                m.push(at);
                                adjust.selected = Some(p.handles().count() - 1);
                            }
                        }
                        // a click inside a mask picks it (Delete removes it)
                        None => adjust.selected = p.masks.iter().rposition(|m| projection::contains(m, at)).map(|m| p.first_of(m)),
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                let step = if self.shift { 10.0 } else { 1.0 };
                let nudge = match &event.logical_key {
                    Key::Named(NamedKey::ArrowLeft) => [-step / w, 0.0],
                    Key::Named(NamedKey::ArrowRight) => [step / w, 0.0],
                    Key::Named(NamedKey::ArrowUp) => [0.0, -step / h],
                    Key::Named(NamedKey::ArrowDown) => [0.0, step / h],
                    _ => [0.0; 2],
                };
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => finish = Some(!adjust.drawing),
                    Key::Named(NamedKey::Enter) => finish = Some(false),
                    Key::Named(NamedKey::Tab) => adjust.selected = Some(adjust.selected.map_or(0, |i| i + 1) % p.handles().count()),
                    Key::Named(NamedKey::Backspace | NamedKey::Delete) => {
                        if let Some(m) = adjust.selected.and_then(|i| p.mask_of(i)) {
                            p.masks.remove(m);
                            (adjust.selected, adjust.drawing) = (None, false);
                        }
                    }
                    _ if nudge != [0.0; 2] => {
                        if let Some(q) = adjust.selected.and_then(|i| p.handle_mut(i)) {
                            *q = [(q[0] + nudge[0]).clamp(0.0, 1.0), (q[1] + nudge[1]).clamp(0.0, 1.0)];
                        }
                    }
                    _ => return false,
                }
            }
            _ => return false,
        }
        shared.adjust = Some(adjust);
        if let Some(stop) = finish {
            shared.finish(stop);
        }
        true
    }

    fn list(&self, event_loop: &ActiveEventLoop) {
        let displays = event_loop.available_monitors().map(|m| Display { name: m.name().unwrap_or_default(), size: [m.size().width, m.size().height], hz: m.refresh_rate_millihertz().map(|mhz| mhz as f32 / 1000.0) }).collect();
        self.shared.borrow_mut().displays = displays;
    }

    fn open(&mut self, event_loop: &ActiveEventLoop, target: Target) {
        // already there: picked for the projection's settings
        if self.outputs.iter().any(|o| o.target == target) {
            self.shared.borrow_mut().picked = Some(target);
            return;
        }
        let Some(engine) = self.engine.clone() else { return };
        let attributes = Window::default_attributes().with_title("Velo output");
        let monitor = match &target {
            Target::Display(name) => match event_loop.available_monitors().find(|m| m.name().as_ref() == Some(name)) {
                Some(m) => Some(m),
                None => return, // unplugged since the menu listed it
            },
            Target::Window => None,
        };
        let attributes = match &monitor {
            // on the main display (the menu bar's, usually Velo's own) it stays a normal window,
            // so Cmd+` brings the UI back over it; elsewhere it stays on top
            Some(m) => {
                let level = if Some(m) == event_loop.primary_monitor().as_ref() { WindowLevel::Normal } else { WindowLevel::AlwaysOnTop };
                attributes.with_decorations(false).with_position(m.position()).with_inner_size(m.size()).with_window_level(level).with_active(false)
            }
            None => {
                let attributes = attributes.with_inner_size(LogicalSize::new(640.0, 360.0));
                // screenshot runs: out of the way, like the UI window
                if std::env::var_os("VELO_CAPTURE_TO").is_some() { attributes.with_position(winit::dpi::LogicalPosition::new(-6000.0, 1000.0)).with_active(false) } else { attributes }
            }
        };
        let Ok(window) = event_loop.create_window(attributes) else { return };
        let window = Arc::new(window);
        // the surface is made here, on the main thread; the render thread configures and presents it
        let Ok(surface) = engine.instance.create_surface(window.clone()) else { return };
        let size = window.inner_size();
        let Some(mut config) = surface.get_default_config(&engine.adapter, size.width.max(1), size.height.max(1)) else { return };
        // display values go out untouched: a non-sRGB format when there is one
        config.format = surface.get_capabilities(&engine.adapter).formats.into_iter().find(|f| !f.is_srgb()).unwrap_or(config.format);
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 1;
        let millihertz = monitor.or_else(|| window.current_monitor()).and_then(|m| m.refresh_rate_millihertz()).filter(|&m| m > 0).unwrap_or(60_000);
        let id = self.next;
        self.next += 1;
        engine.send(Command::Attach { id, surface, config, period: Duration::from_secs_f64(1000.0 / f64::from(millihertz)) });
        self.outputs.push(Open { target: target.clone(), window, id, sent: None });
        let mut shared = self.shared.borrow_mut();
        shared.outputs.push(target.clone());
        shared.picked = Some(target);
    }

    /// A new engine for one that stopped: its windows go at once (the old render thread may never
    /// let go of them), the outputs come back on the same displays.
    fn restart(&mut self, event_loop: &ActiveEventLoop) {
        let targets = std::mem::take(&mut self.shared.borrow_mut().outputs);
        (self.outputs, self.closing) = (Vec::new(), Vec::new());
        let fresh = Engine::start(Some(Box::new(event_loop.owned_display_handle()))).map(Rc::new);
        self.engine = fresh.as_ref().ok().cloned();
        if fresh.is_ok() {
            for target in targets {
                self.open(event_loop, target);
            }
        }
        self.shared.borrow_mut().fresh = Some(fresh);
    }

    /// Closes the output going there, or every one (None).
    fn close(&mut self, target: Option<&Target>) {
        let (gone, kept) = std::mem::take(&mut self.outputs).into_iter().partition(|o| target.is_none_or(|t| &o.target == t));
        self.outputs = kept;
        let gone: Vec<Open> = gone;
        let mut shared = self.shared.borrow_mut();
        for o in gone {
            if let Some(engine) = &self.engine {
                engine.send(Command::Detach(o.id));
                self.closing.push((o.id, o.window));
            }
            // closing the output being adjusted ends adjusting
            if shared.adjust.is_some() && shared.current() == Some(&o.target) {
                shared.finish(true);
            }
            shared.outputs.retain(|t| t != &o.target);
        }
    }

    /// Each output's projection to the engine, when it changed; the handles on the current one
    /// while it's adjusted.
    fn send_projections(&mut self) {
        let shared = self.shared.borrow();
        let current = shared.current().cloned();
        for o in &mut self.outputs {
            let guides = shared.adjust.filter(|_| current.as_ref() == Some(&o.target)).map(|a| a.selected);
            let want = (shared.projections.get(&key(&o.target)).cloned().unwrap_or_default(), guides);
            if let Some(engine) = &self.engine
                && o.sent.as_ref() != Some(&want)
            {
                engine.send(Command::Projection { id: o.id, projection: want.0.clone(), guides });
                o.sent = Some(want);
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for Host<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.resumed(event_loop);
        self.list(event_loop);
        // VELO_OUTPUT=window: a windowed output from the start (tests of the output path)
        if std::env::var("VELO_OUTPUT").as_deref() == Ok("window") && self.outputs.is_empty() {
            self.open(event_loop, Target::Window);
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.eframe.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        self.eframe.user_event(event_loop, event);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.closing.iter().any(|w| w.1.id() == id) {
            return;
        }
        let Some((output, target)) = self.outputs.iter().find(|o| o.window.id() == id).map(|o| (o.id, o.target.clone())) else {
            return self.eframe.window_event(event_loop, id, event);
        };
        match &event {
            WindowEvent::CursorMoved { position, .. } => self.cursor = [position.x as f32, position.y as f32],
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            _ => {}
        }
        if self.adjust(id, &event) {
            return;
        }
        // an output window takes one key: Esc closes it (when it has the keyboard, so a key
        // in the main window never does)
        match event {
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() && event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => self.close(Some(&target)),
            WindowEvent::Resized(size) => {
                if let Some(engine) = &self.engine {
                    engine.send(Command::Resize { id: output, size: [size.width, size.height] });
                }
            }
            WindowEvent::Occluded(hidden) => {
                if let Some(engine) = &self.engine {
                    engine.send(Command::Occluded { id: output, hidden });
                }
            }
            WindowEvent::CloseRequested => self.close(Some(&target)),
            _ => {}
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, device: DeviceId, event: DeviceEvent) {
        self.eframe.device_event(event_loop, device, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.about_to_wait(event_loop);
        // once a second: an output's display unplugged closes that output (Windows would move
        // its window over the UI's, on top)
        if self.looked.elapsed() > Duration::from_secs(1) {
            self.looked = std::time::Instant::now();
            let names: Vec<String> = event_loop.available_monitors().filter_map(|m| m.name()).collect();
            let gone: Vec<Target> = self.outputs.iter().map(|o| o.target.clone()).filter(|t| matches!(t, Target::Display(name) if !names.contains(name))).collect();
            for target in gone {
                self.close(Some(&target));
                if let Target::Display(name) = target {
                    self.shared.borrow_mut().unplugged = Some(name);
                }
            }
        }
        let requests = std::mem::take(&mut self.shared.borrow_mut().requests);
        for request in requests {
            match request {
                Request::Open(target) => self.open(event_loop, target),
                Request::Close(target) => self.close(target.as_ref()),
                Request::Displays => self.list(event_loop),
                Request::Restart => self.restart(event_loop),
            }
        }
        self.send_projections();
        if let Some(engine) = &self.engine {
            for id in engine.detached.try_iter() {
                self.closing.retain(|w| w.0 != id);
            }
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(engine) = &self.engine {
            engine.send(Command::Quit);
        }
        self.eframe.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.memory_warning(event_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_left_unfinished_goes_and_each_output_keeps_its_own() {
        let mut s = Shared { outputs: vec![Target::Display("EPSON PJ".into())], ..Default::default() };
        s.draw_mask();
        assert!(s.adjust.is_some_and(|a| a.drawing));
        s.projection().expect("an output").masks.last_mut().expect("the new mask").extend([[0.1, 0.1], [0.2, 0.1]]);
        s.finish(false);
        assert!(s.projections["EPSON PJ"].masks.is_empty(), "two points mask nothing: dropped");
        assert!(s.adjust.is_some_and(|a| !a.drawing), "still adjusting");
        s.projection().expect("an output").corners[0] = [0.1, 0.1];
        s.outputs.push(Target::Window);
        assert_eq!(s.projection().expect("an output").corners, projection::SQUARE, "the window, opened last, has its own");
        s.finish(true);
        assert!(s.adjust.is_none());
    }

    #[test]
    fn the_projection_set_is_the_picked_outputs_while_it_is_open() {
        let [a, b] = [Target::Display("Left PJ".into()), Target::Display("Right PJ".into())];
        let mut s = Shared { outputs: vec![a.clone(), b.clone()], ..Default::default() };
        assert_eq!(s.current(), Some(&b), "the last opened");
        s.picked = Some(a.clone());
        s.projection().expect("an output").turn = 1;
        assert_eq!((s.projections["Left PJ"].turn, s.projections.contains_key("Right PJ")), (1, false));
        // the picked one closed: back to the last opened
        s.outputs.retain(|t| t != &a);
        assert_eq!(s.current(), Some(&b));
        s.outputs.clear();
        assert!(s.projection().is_none());
    }
}
