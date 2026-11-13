//! Velo's engine (ARCHITECTURE.md): shader nodes, graphs compiled on a worker, the render
//! thread with its outputs and preview (on the spike S1 design), the modulation it runs
//! every frame (mappings, the beat clock) and scene launches (quantized, cut or crossfade).

pub mod compile;
pub mod effects;
pub mod gpu;
pub mod hap;
pub mod input;
pub mod isf;
pub mod launch;
pub mod media;
pub mod model;
pub mod modulate;
pub mod presets;
pub mod projection;
pub mod runtime;
pub mod shader;
pub mod share;
pub mod signal;
pub mod text;
#[cfg(target_os = "macos")]
pub mod video;
#[cfg(windows)]
#[path = "video_windows.rs"]
pub mod video;

/// GPU tests take turns: several devices made and driven at once can stall a driver (seen on
/// Windows, a test spinning for minutes).
#[cfg(test)]
pub(crate) fn gpu_lock() -> std::sync::MutexGuard<'static, ()> {
    static GPU: std::sync::Mutex<()> = std::sync::Mutex::new(());
    GPU.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub use compile::{Built, Code, Compiler, EFFECT_TEMPLATE, Plan, TEMPLATE, builtin, own, plan, template};
pub use effects::EFFECT_PRESETS;
pub use gpu::{NodeSpec, Program, Runner};
pub use launch::Show;
pub use modulate::{Clock, MacroRoute, Macros, Modulator, Route, macro_routes, routes};
pub use presets::PRESETS;
pub use projection::Projection;
/// A wgpu error as one line to show: its innermost cause.
pub fn gpu_error(e: &wgpu::Error) -> String {
    let mut text = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        text = s.to_string();
        source = s.source();
    }
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or("the GPU refused it").trim().to_string()
}

pub use runtime::{Command, Engine, Graph, PREVIEW, SharedFeatures, SharedWave, Status, WAVE_COLUMNS};
pub use shader::{BUILTINS, GLOBALS, ParamType, ShaderError, ShaderNode};
