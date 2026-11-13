//! Our event loop (spike S1): eframe runs as an `ApplicationHandler` inside it
//! (`eframe::create_native`), so the output window is ours. The main thread creates output
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
    Open(Target),
    Close,
    /// List the displays again (the output menu opened).
    Displays,
    /// Start the engine again (the GPU was lost, or the render thread stopped drawing): the
    /// output reopens where it was.
    Restart,
}

/// What the UI and the event loop share. Both live on the main thread.
#[derive(Default)]
pub struct Shared {
    pub requests: Vec<Request>,
    pub displays: Vec<Display>,
    /// Where the output is now: set by the event loop once the window exists.
    pub output: Option<Target>,
    /// A restarted engine, for the UI to take up (or why it didn't start).
    pub fresh: Option<Result<Rc<Engine>, String>>,
    /// The display the output was on, unplugged: the output closed, for the UI to say.
    pub unplugged: Option<String>,
    /// Each output's corner pin and masks, by where it goes ([`key`]): the UI keeps them with
    /// the preferences, the event loop sends the one in use to the engine.
    pub projections: std::collections::BTreeMap<String, Projection>,
    /// Adjusting the projection: handles on the output, moved with the mouse and keys there.
    pub adjust: Option<Adjust>,
}

impl Shared {
    /// The output in use's corner pin and masks, made if it has none.
    pub fn projection(&mut self) -> Option<&mut Projection> {
        let key = key(self.output.as_ref()?);
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

pub struct Host<'a> {
    pub eframe: eframe::EframeWinitApplication<'a>,
    pub engine: Option<Rc<Engine>>,
    pub shared: Rc<RefCell<Shared>>,
    output: Option<Arc<Window>>,
    /// Closed outputs, dropped here on the main thread once the render thread let go of them.
    closing: Vec<Arc<Window>>,
    /// When the output's display was last looked for.
    looked: std::time::Instant,
    /// The mouse on the output window, in pixels, and Shift held there.
    cursor: [f32; 2],
    shift: bool,
    /// The projection the engine was last sent, with its handles (None: nothing sent yet).
    sent: Option<(Projection, Option<Option<usize>>)>,
}

impl<'a> Host<'a> {
    pub fn new(eframe: eframe::EframeWinitApplication<'a>, engine: Option<Rc<Engine>>, shared: Rc<RefCell<Shared>>) -> Self {
        Self { eframe, engine, shared, output: None, closing: Vec::new(), looked: std::time::Instant::now(), cursor: [0.0; 2], shift: false, sent: None }
    }

    /// The mouse and keys on the output window while the projection is adjusted: drag a handle,
    /// click to add a mask's point, arrows nudge the picked handle a pixel (Shift: ten). True
    /// when the event was theirs.
    fn adjust(&mut self, event: &WindowEvent) -> bool {
        use winit::keyboard::{Key, NamedKey};
        let Some(window) = &self.output else { return false };
        let size = window.inner_size();
        let [w, h] = [size.width.max(1) as f32, size.height.max(1) as f32];
        let mut shared = self.shared.borrow_mut();
        let Some(mut adjust) = shared.adjust else { return false };
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
        self.close();
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
        let millihertz = monitor.or_else(|| window.current_monitor()).and_then(|m| m.refresh_rate_millihertz()).unwrap_or(60_000);
        engine.send(Command::Attach { surface, config, period: Duration::from_secs_f64(1000.0 / f64::from(millihertz)) });
        self.output = Some(window);
        self.shared.borrow_mut().output = Some(target);
    }

    /// A new engine for one that stopped: its windows go at once (the old render thread may never
    /// let go of them), the output comes back on the same display.
    fn restart(&mut self, event_loop: &ActiveEventLoop) {
        let target = self.shared.borrow_mut().output.take();
        (self.output, self.closing, self.sent) = (None, Vec::new(), None);
        let fresh = Engine::start(Some(Box::new(event_loop.owned_display_handle()))).map(Rc::new);
        self.engine = fresh.as_ref().ok().cloned();
        if let Some(target) = target.filter(|_| fresh.is_ok()) {
            self.open(event_loop, target);
        }
        self.shared.borrow_mut().fresh = Some(fresh);
    }

    fn close(&mut self) {
        if let (Some(window), Some(engine)) = (self.output.take(), &self.engine) {
            engine.send(Command::Detach);
            self.closing.push(window);
        }
        let mut shared = self.shared.borrow_mut();
        // an output closing ends adjusting it (not opening one, which closes none first)
        if shared.output.is_some() && shared.adjust.is_some() {
            shared.finish(true);
        }
        shared.output = None;
        self.sent = None;
    }

    /// The output in use's projection to the engine, when it changed.
    fn send_projection(&mut self) {
        let shared = self.shared.borrow();
        let want = shared.output.as_ref().map(|t| (shared.projections.get(&key(t)).cloned().unwrap_or_default(), shared.adjust.map(|a| a.selected)));
        if let (Some((projection, guides)), Some(engine)) = (&want, &self.engine)
            && want != self.sent
        {
            engine.send(Command::Projection { projection: projection.clone(), guides: *guides });
        }
        drop(shared);
        self.sent = want;
    }
}

impl ApplicationHandler<UserEvent> for Host<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.resumed(event_loop);
        self.list(event_loop);
        // VELO_OUTPUT=window: a windowed output from the start (tests of the output path)
        if std::env::var("VELO_OUTPUT").as_deref() == Ok("window") && self.output.is_none() {
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
        if self.closing.iter().any(|w| w.id() == id) {
            return;
        }
        if self.output.as_ref().is_none_or(|w| w.id() != id) {
            return self.eframe.window_event(event_loop, id, event);
        }
        match &event {
            WindowEvent::CursorMoved { position, .. } => self.cursor = [position.x as f32, position.y as f32],
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            _ => {}
        }
        if self.adjust(&event) {
            return;
        }
        // the output window takes one key: Esc closes it (when it has the keyboard, so a key
        // in the main window never does)
        match event {
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() && event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) => self.close(),
            WindowEvent::Resized(size) => {
                if let Some(engine) = &self.engine {
                    engine.send(Command::Resize([size.width, size.height]));
                }
            }
            WindowEvent::Occluded(hidden) => {
                if let Some(engine) = &self.engine {
                    engine.send(Command::Occluded(hidden));
                }
            }
            WindowEvent::CloseRequested => self.close(),
            _ => {}
        }
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, device: DeviceId, event: DeviceEvent) {
        self.eframe.device_event(event_loop, device, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.eframe.about_to_wait(event_loop);
        // once a second: the output's display unplugged closes the output (Windows would move its
        // window over the UI's, on top)
        if self.looked.elapsed() > Duration::from_secs(1) {
            self.looked = std::time::Instant::now();
            let target = self.shared.borrow().output.clone();
            if let Some(Target::Display(name)) = target
                && !event_loop.available_monitors().any(|m| m.name().as_ref() == Some(&name))
            {
                self.close();
                self.shared.borrow_mut().unplugged = Some(name);
            }
        }
        let requests = std::mem::take(&mut self.shared.borrow_mut().requests);
        for request in requests {
            match request {
                Request::Open(target) => self.open(event_loop, target),
                Request::Close => self.close(),
                Request::Displays => self.list(event_loop),
                Request::Restart => self.restart(event_loop),
            }
        }
        self.send_projection();
        if let Some(engine) = &self.engine {
            for () in engine.detached.try_iter() {
                if !self.closing.is_empty() {
                    self.closing.remove(0);
                }
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
        let mut s = Shared { output: Some(Target::Display("EPSON PJ".into())), ..Default::default() };
        s.draw_mask();
        assert!(s.adjust.is_some_and(|a| a.drawing));
        s.projection().expect("an output").masks.last_mut().expect("the new mask").extend([[0.1, 0.1], [0.2, 0.1]]);
        s.finish(false);
        assert!(s.projections["EPSON PJ"].masks.is_empty(), "two points mask nothing: dropped");
        assert!(s.adjust.is_some_and(|a| !a.drawing), "still adjusting");
        s.projection().expect("an output").corners[0] = [0.1, 0.1];
        s.output = Some(Target::Window);
        assert_eq!(s.projection().expect("an output").corners, projection::SQUARE, "the window has its own");
        s.finish(true);
        assert!(s.adjust.is_none());
    }
}
