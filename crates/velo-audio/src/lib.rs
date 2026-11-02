//! Velo's ears (docs/audio.md): audio input from any device or the computer's own output, and
//! realtime analysis that turns it into levels, bands and onsets.
//!
//! No GPU, no UI: the analysis runs on plain sample slices, so tests and the offline harness use
//! exactly the code the app runs.

pub mod analysis;
pub mod apps;
pub mod capture;
pub mod drums;
pub mod features;
pub mod spectrum;
pub mod synth;
pub mod tempo;

pub use analysis::{Analyzer, BANDS, Detect, HITS, HOP, Hop, Mode, Settings};
pub use capture::{Capture, Source};
pub use features::{Features, Tracker};
pub use tempo::{Estimate, Tempo};
