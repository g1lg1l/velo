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
}

impl<'a> Host<'a> {
    pub fn new(eframe: eframe::EframeWinitApplication<'a>, engine: Option<Rc<Engine>>, shared: Rc<RefCell<Shared>>) -> Self {
        Self { eframe, engine, shared, output: None, closing: Vec::new(), looked: std::time::Instant::now() }
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
        (self.output, self.closing) = (None, Vec::new());
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
        self.shared.borrow_mut().output = None;
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
