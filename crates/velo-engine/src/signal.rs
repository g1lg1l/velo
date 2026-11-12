//! Signal nodes (docs/graph.md §3): numbers, not images, computed on the render thread each
//! frame and handed to mappings as sources, like Kick or Bass. An Audio In puts what the input
//! hears on the canvas, a Beat the clock; a Beat Kick hits on the beat as hard as the bass, not
//! at all while the bass is low; an LFO is a shape over a number of beats; a Remap
//! brings a signal into the range wanted; a Math puts two together; a Gain makes one stronger; an Envelope plays a shape
//! on each hit; a Counter steps on each hit; a Random picks a value on the beat; a Smooth
//! follows its input slowly; a Quantize snaps it to steps; a Speed goes round a cycle as fast
//! as its input says; a Delay plays its input back some beats later; a Probe reads a picture
//! (its color comes back from the GPU, [`crate::gpu::Probe`]). Each output has a slot after the
//! shared sources; a mapping names it as "node id/output". The project's shared signal graph
//! runs once a frame into slots of its own after every scene's ([`SHARED`]), which any graph
//! reads ("shared:node id/output").

use std::collections::VecDeque;
use std::f64::consts::TAU;

use velo_core::{AUDIO, BEAT_KICK, CLOCK, COMPARE, COUNTER, Category, DELAY, ENVELOPE, GAIN, HOLD, LFO, MATH, PROBE, QUANTIZE, RANDOM, REMAP, SEQUENCER, SLOPE, SMOOTH, SOURCES, SPEED, SPRING, Scene, TIME, TIMER, TOGGLE, node_kind};

use crate::shader::{Param, ParamType};

/// Signal outputs a graph can have (their values sit after the audio and clock sources).
pub const MAX_SIGNALS: usize = 64;
/// Where the shared signal graph's outputs start among the sources: after the audio and clock
/// sources and a scene's own signals, so a scene's pass never writes over them.
pub const SHARED: usize = SOURCES.len() + MAX_SIGNALS;
/// Every source a frame has: audio and clock, a scene's signals, the shared ones.
pub const SLOTS: usize = SHARED + MAX_SIGNALS;

pub use velo_core::LFO_SHAPES as SHAPES;

/// A signal node's parameters, described as a shader's are (so the inspector, MIDI and OSC
/// handle them like any node's); none for a kind without any, or that isn't a signal node.
pub fn params(kind: &str) -> Option<Vec<Param>> {
    let p = |id: &str, ty, default: f32, min: f32, max: f32| Param { id: id.into(), ty, default: [default, 0.0, 0.0, 0.0], min, max, names: Vec::new() };
    let named = |id: &str, names: &[&str]| Param { names: names.iter().map(|n| n.to_string()).collect(), ..p(id, ParamType::Int, 0.0, 0.0, (names.len() - 1) as f32) };
    let float = ParamType::Float;
    // an Audio In's hit: how far its band must rise (dB), the band (Hz), the shortest gap; in
    // Model mode how sure the model must be
    let hit = |name: &str, threshold: f32, low: f32, high: f32, gap_ms: f32, confidence: f32| {
        [
            p(&format!("{name}_threshold"), float, threshold, 1.0, 30.0),
            p(&format!("{name}_low"), float, low, 20.0, 20_000.0),
            p(&format!("{name}_high"), float, high, 20.0, 20_000.0),
            p(&format!("{name}_gap_ms"), float, gap_ms, 10.0, 1000.0),
            p(&format!("{name}_confidence"), float, confidence, 0.05, 0.95),
        ]
    };
    Some(match kind {
        // the defaults are velo_audio::Settings::default()'s (the app's tests check)
        AUDIO => [Param { default: [1.0, 0.0, 0.0, 0.0], ..named("mode", &DETECT_MODES) }, p("gate", float, -60.0, -90.0, -20.0)]
            .into_iter()
            .chain(hit("kick", 3.0, 70.0, 300.0, 100.0, 0.45))
            .chain(hit("snare", 8.0, 1000.0, 8000.0, 80.0, 0.35))
            .chain(hit("hat", 4.0, 8000.0, 20_000.0, 50.0, 0.55))
            .collect(),
        // no kick while the bass is under a third; a quarter second to fall
        BEAT_KICK => vec![p("threshold", float, 0.3, 0.0, 1.0), p("release_ms", float, 250.0, 20.0, 2000.0)],
        LFO => vec![named("shape", &SHAPES), p("beats", float, 4.0, 0.25, 64.0), p("phase", float, 0.0, 0.0, 1.0)],
        REMAP => vec![
            p("auto", ParamType::Bool, 1.0, 0.0, 1.0),
            p("in_min", float, 0.0, 0.0, 1.0),
            p("in_max", float, 1.0, 0.0, 1.0),
            p("out_min", float, 0.0, 0.0, 1.0),
            p("out_max", float, 1.0, 0.0, 1.0),
            named("curve", &CURVES),
            p("gate", float, 0.0, 0.0, 1.0),
            p("attack_ms", float, 5.0, 1.0, 1000.0),
            p("release_ms", float, 150.0, 1.0, 2000.0),
            p("invert", ParamType::Bool, 0.0, 0.0, 1.0),
        ],
        MATH => vec![named("operation", &MATH_OPS), p("value", float, 0.5, 0.0, 1.0)],
        // twice as strong to start with; a Math's value stops at 1, so it can't amplify
        GAIN => vec![p("gain", float, 2.0, 0.0, GAIN_MAX)],
        TIME => vec![p("seconds", float, 10.0, 0.5, 600.0), named("shape", &TIME_SHAPES)],
        ENVELOPE => vec![p("threshold", float, 0.5, 0.0, 1.0), p("attack_ms", float, 5.0, 1.0, 2000.0), p("release_ms", float, 300.0, 1.0, 4000.0), named("curve", &CURVES)],
        COUNTER => vec![p("steps", ParamType::Int, 4.0, 2.0, 16.0), named("mode", &COUNT_MODES), p("threshold", float, 0.5, 0.0, 1.0)],
        // a hit on the first step of every two beats to start from
        SEQUENCER => [p("steps", ParamType::Int, 8.0, 2.0, 16.0), Param { default: [2.0, 0.0, 0.0, 0.0], ..named("rate", &SEQ_RATES) }]
            .into_iter()
            .chain((1..=16).map(|i| p(&format!("step_{i}"), float, if i % 4 == 1 { 1.0 } else { 0.0 }, 0.0, 1.0)))
            .collect(),
        HOLD | TOGGLE => vec![p("threshold", float, 0.5, 0.0, 1.0)],
        COMPARE => vec![named("operation", &COMPARE_OPS), p("value", float, 0.5, 0.0, 1.0)],
        TIMER => vec![p("seconds", float, 4.0, 0.1, 600.0)],
        RANDOM => vec![p("beats", float, 1.0, 0.25, 64.0), p("glide_ms", float, 1.0, 1.0, 10_000.0), p("threshold", float, 0.5, 0.0, 1.0)],
        SMOOTH => vec![named("mode", &SMOOTH_MODES), p("rise_ms", float, 300.0, 1.0, 10_000.0), p("fall_ms", float, 300.0, 1.0, 10_000.0)],
        QUANTIZE => vec![p("steps", ParamType::Int, 4.0, 2.0, 32.0)],
        // a turn every 4 s at full speed
        SPEED => vec![p("seconds", float, 4.0, 0.1, 600.0), named("shape", &TIME_SHAPES)],
        DELAY => vec![p("beats", float, 1.0, 0.25, DELAY_BEATS)],
        // a swing of half a second, overshooting by about a fifth
        SPRING => vec![p("time_ms", float, 500.0, 50.0, 4000.0), p("bounce", float, 0.5, 0.0, 0.95)],
        // the whole range in a tenth of a second is 1
        SLOPE => vec![named("direction", &SLOPE_DIRECTIONS), p("time_ms", float, 100.0, 10.0, 2000.0)],
        // the whole picture to start with
        PROBE => vec![named("measure", &PROBE_MEASURES), p("position", ParamType::Vec2, 0.0, -1.0, 1.0), p("size", float, 1.0, 0.0, 1.0)],
        _ => return None,
    })
}

/// Where an Audio In's hits come from, in its `mode` parameter's order: its band detectors, or
/// the drum model.
pub const DETECT_MODES: [&str; 2] = ["Simple", "Model"];
/// A Remap's and an Envelope's curves, in their `curve` parameter's order.
pub const CURVES: [&str; 4] = ["Linear", "Ease in", "Ease out", "Smooth"];
/// A Math node's operations, in its `op` parameter's order.
pub const MATH_OPS: [&str; 7] = ["Add", "Subtract", "Multiply", "Min", "Max", "Average", "Difference"];
/// How Time goes through each cycle, in its `shape` parameter's order.
pub const TIME_SHAPES: [&str; 3] = ["Ramp", "Back and forth", "Wave"];
/// A Counter's ways through its steps, in its `mode` parameter's order.
pub const COUNT_MODES: [&str; 4] = ["Up", "Down", "Ping-pong", "Random"];
/// How long a Sequencer's step is, in its `rate` parameter's order, and in beats.
pub const SEQ_RATES: [&str; 5] = ["1/4 beat", "1/2 beat", "Beat", "2 beats", "Bar"];
pub const SEQ_BEATS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];
/// A Compare's ways, in its `operation` parameter's order.
pub const COMPARE_OPS: [&str; 2] = ["Above", "Below"];
/// How a Smooth follows, in its `mode` parameter's order: at a steady speed, or slowing as it nears.
pub const SMOOTH_MODES: [&str; 2] = ["Glide", "Lag"];
/// Which way a Slope counts change, in its `direction` parameter's order.
pub const SLOPE_DIRECTIONS: [&str; 3] = ["Rising", "Falling", "Either way"];
/// What a Probe reads of the color, in its `measure` parameter's order.
pub const PROBE_MEASURES: [&str; 5] = ["Brightness", "Red", "Green", "Blue", "Alpha"];

/// The most a Gain multiplies its input by.
pub const GAIN_MAX: f32 = 16.0;

/// The longest a Delay holds its input back, in beats.
pub const DELAY_BEATS: f32 = 16.0;

/// Where `x` (0..1) is in a cycle, through one of [`TIME_SHAPES`].
fn cycle(shape: u8, x: f32) -> f32 {
    match shape {
        1 => 1.0 - (2.0 * x - 1.0).abs(),
        2 => 0.5 - 0.5 * (std::f32::consts::TAU * x).cos(),
        _ => x,
    }
}

/// `x` as it was `back` beats ago on the clock (0 till the clock has gone that far). `history`
/// keeps (clock, value) [`DELAY_BEATS`] back, so a delay made longer finds what it needs.
fn delay(x: f32, beats: f64, back: f32, history: &mut VecDeque<(f64, f32)>) -> f32 {
    // the clock went back (a resync): what's kept is from another time
    if history.back().is_some_and(|&(b, _)| b > beats) {
        history.clear();
    }
    // a clock standing still keeps one value, not a frame's each
    if let Some(last) = history.back_mut().filter(|(b, _)| *b == beats) {
        last.1 = x;
    } else {
        history.push_back((beats, x));
    }
    while history.get(1).is_some_and(|&(b, _)| b <= beats - f64::from(DELAY_BEATS)) {
        history.pop_front();
    }
    match history.partition_point(|&(b, _)| b <= beats - f64::from(back)) {
        0 => 0.0,
        i => history[i - 1].1,
    }
}

/// `v` (0..1) through one of [`CURVES`].
fn curve(curve: u8, v: f32) -> f32 {
    match curve {
        1 => v * v,
        2 => 1.0 - (1.0 - v) * (1.0 - v),
        3 => v * v * (3.0 - 2.0 * v),
        _ => v,
    }
}

/// One LFO: its shape, its length in beats, where it starts in that cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: u8,
    pub beats: f32,
    pub phase: f32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self { shape: 0, beats: 4.0, phase: 0.0 }
    }
}

impl Lfo {
    /// From a node's parameter values (by id; the rest take their defaults).
    pub fn from_params(get: impl Fn(&str) -> Option<f32>) -> Self {
        let d = Self::default();
        Self { shape: get("shape").map_or(d.shape, |s| s.round().clamp(0.0, 4.0) as u8), beats: get("beats").map_or(d.beats, |b| b.clamp(0.25, 64.0)), phase: get("phase").map_or(d.phase, |p| p.clamp(0.0, 1.0)) }
    }

    /// Its value (0..1) `beats` into the clock.
    pub fn at(&self, beats: f64) -> f32 {
        let cycles = beats / f64::from(self.beats.max(0.25)) + f64::from(self.phase);
        let x = cycles.rem_euclid(1.0);
        (match self.shape {
            0 => 0.5 - 0.5 * (TAU * x).cos(),
            1 => 1.0 - (2.0 * x - 1.0).abs(),
            2 => x,
            3 => f64::from(u8::from(x < 0.5)),
            // a new random value each cycle, held
            _ => {
                let h = (cycles.floor() * 12.9898).sin() * 43758.5453;
                h - h.floor()
            }
        }) as f32
    }
}

/// A signal brought into a range: from its own (learned, with `auto`, else `input`) to `output`,
/// through a curve, below `gate` nothing, smoothed (attack when it rises, release when it falls).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Remap {
    pub auto: bool,
    pub input: [f32; 2],
    pub output: [f32; 2],
    pub curve: u8,
    pub gate: f32,
    /// Seconds.
    pub attack: f32,
    pub release: f32,
    pub invert: bool,
}

impl Remap {
    pub fn from_params(get: impl Fn(&str) -> Option<f32>) -> Self {
        let v = |id: &str, d: f32| get(id).unwrap_or(d);
        Self {
            auto: v("auto", 1.0) >= 0.5,
            input: [v("in_min", 0.0), v("in_max", 1.0)],
            output: [v("out_min", 0.0), v("out_max", 1.0)],
            curve: v("curve", 0.0).round().clamp(0.0, 3.0) as u8,
            gate: v("gate", 0.0),
            attack: v("attack_ms", 5.0).max(0.0) / 1000.0,
            release: v("release_ms", 150.0).max(0.0) / 1000.0,
            invert: v("invert", 0.0) >= 0.5,
        }
    }

    /// One frame of input `x`, `dt` seconds after the last; `state` is (level, learned low,
    /// learned high), kept between frames.
    pub fn step(&self, x: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
        let [level, lo, hi, _] = state;
        // the range it moves in: out at once, back in over seconds (a quiet passage widens it again)
        if *lo > *hi {
            (*lo, *hi) = (x, x);
        }
        let back = 1.0 - (-dt / 8.0).exp();
        *lo = if x < *lo { x } else { *lo + (x - *lo) * back };
        *hi = if x > *hi { x } else { *hi + (x - *hi) * back };
        let [a, b] = if self.auto { [*lo, *hi] } else { self.input };
        let mut v = if (b - a).abs() > 1e-4 { ((x - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        if v < self.gate {
            v = 0.0;
        }
        v = curve(self.curve, v);
        if self.invert {
            v = 1.0 - v;
        }
        let tau = if v > *level { self.attack } else { self.release };
        *level += (v - *level) * if tau > 0.0 { 1.0 - (-dt / tau).exp() } else { 1.0 };
        self.output[0] + (self.output[1] - self.output[0]) * *level
    }
}

/// Whether a signal went over `threshold` since the last frame (it was under it, `last`).
fn hit(x: f32, last: &mut f32, threshold: f32) -> bool {
    let rose = *last < threshold && x >= threshold;
    *last = x;
    rose
}

/// Two signals into one: a and b (b a fixed `value` while nothing's wired into it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Math {
    pub op: u8,
    pub value: f32,
}

impl Math {
    pub fn at(&self, a: f32, b: f32) -> f32 {
        (match self.op {
            1 => a - b,
            2 => a * b,
            3 => a.min(b),
            4 => a.max(b),
            5 => (a + b) / 2.0,
            6 => (a - b).abs(),
            _ => a + b,
        })
        .clamp(0.0, 1.0)
    }
}

/// On each hit (the input going over `threshold`), a rise to 1 over `attack` seconds and a fall
/// back over `release`, through a curve; a hit while it plays starts it again from where it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Envelope {
    pub threshold: f32,
    pub attack: f32,
    pub release: f32,
    pub curve: u8,
}

impl Envelope {
    /// One frame; `state` is (level, last input, rising: 1 or 0).
    pub fn step(&self, x: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
        let [level, last, rising, _] = state;
        if hit(x, last, self.threshold) {
            *rising = 1.0;
        }
        if *rising > 0.0 {
            *level += dt / self.attack.max(1e-3);
            if *level >= 1.0 {
                (*level, *rising) = (1.0, 0.0);
            }
        } else {
            *level = (*level - dt / self.release.max(1e-3)).max(0.0);
        }
        curve(self.curve, *level)
    }
}

/// A step on each hit, through `steps` values (0..1) as `mode` says; a hit on `reset` goes back
/// to the first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Counter {
    pub steps: u8,
    pub mode: u8,
    pub threshold: f32,
}

impl Counter {
    /// One frame; `state` is (hits so far, last input, the step it's on, last reset).
    pub fn step(&self, x: f32, reset: f32, state: &mut [f32; 4]) -> f32 {
        let [hits, last, at, last_reset] = state;
        let steps = f32::from(self.steps.max(2));
        if hit(reset, last_reset, self.threshold) {
            (*hits, *at) = (0.0, 0.0);
        }
        if hit(x, last, self.threshold) {
            *hits += 1.0;
            let period = 2.0 * (steps - 1.0);
            *at = match self.mode {
                1 => steps - 1.0 - *hits % steps,
                2 => {
                    let p = *hits % period;
                    if p < steps { p } else { period - p }
                }
                // anywhere but where it is
                3 => {
                    let h = (*hits * 12.9898).sin() * 43758.547;
                    (*at + 1.0 + ((h - h.floor()) * (steps - 1.0)).floor()) % steps
                }
                _ => *hits % steps,
            };
        }
        *at / (steps - 1.0)
    }
}

/// A value a step on the beat clock: `steps` of them, each `beats` long, from the clock's start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sequencer {
    pub steps: u8,
    pub beats: f64,
    pub values: [f32; 16],
}

impl Sequencer {
    pub fn at(&self, beats: f64) -> f32 {
        let step = (beats / self.beats).floor().rem_euclid(f64::from(self.steps.max(1))) as usize;
        self.values[step.min(15)]
    }
}

/// On each hit of `trigger`, `x` held until the next (or, with nothing wired into `in`, a random
/// value, never the same twice); `state` is (held, last trigger, hits so far).
fn hold(x: Option<f32>, trigger: f32, threshold: f32, state: &mut [f32; 4]) -> f32 {
    let [held, last, hits, _] = state;
    if hit(trigger, last, threshold) {
        *hits += 1.0;
        *held = x.unwrap_or_else(|| {
            let h = (*hits * 12.9898).sin() * 43758.547;
            // anywhere but within a tenth of where it was
            (*held + 0.1 + (h - h.floor()) * 0.8).fract()
        });
    }
    *held
}

/// How long a Timer's hit takes to fall back from 1 (s): enough to see, and to cross a threshold.
const TIMER_FALL_S: f32 = 0.15;

/// Seconds going by, again and again: a hit each time `seconds` have passed, 1 falling back over
/// [`TIMER_FALL_S`]; a hit on `reset` starts it over. `state` is (seconds in, last reset, level).
fn timer(seconds: f32, reset: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
    let [elapsed, last, level, _] = state;
    if hit(reset, last, 0.5) {
        *elapsed = 0.0;
    }
    *elapsed += dt;
    *level *= (-dt / TIMER_FALL_S).exp();
    if *elapsed >= seconds {
        *elapsed %= seconds;
        *level = 1.0;
    }
    *level
}

/// A new random value (0..1, never within a tenth of the last) on each hit of its input or, with
/// nothing wired in, every `beats` of the clock, glided to over `glide` seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Random {
    pub beats: f32,
    pub glide: f32,
    pub threshold: f32,
}

impl Random {
    /// One frame; `seed` keeps two Randoms apart; `state` is (from, to, seconds since it picked,
    /// the last input or clock step).
    pub fn step(&self, trigger: Option<f32>, beats: f64, dt: f32, seed: usize, state: &mut [f32; 4]) -> f32 {
        let [from, to, since, last] = state;
        let ease = |since: f32| {
            let k = if self.glide > 0.0 { (since / self.glide).min(1.0) } else { 1.0 };
            k * k * (3.0 - 2.0 * k)
        };
        let new = match trigger {
            Some(x) => hit(x, last, self.threshold),
            None => {
                let step = (beats / f64::from(self.beats.max(0.0625))).floor() as f32;
                let changed = step != *last;
                *last = step;
                changed
            }
        };
        *since += dt;
        if new {
            // from where it is now, to anywhere but within a tenth of where it was going
            *from += (*to - *from) * ease(*since);
            let h = ((f64::from(*to) * 1000.0 + seed as f64 * 7.31) * 12.9898).sin() * 43758.547;
            *to = (*to + 0.1 + (h - h.floor()) as f32 * 0.8).fract();
            *since = 0.0;
        }
        *from + (*to - *from) * ease(*since)
    }
}

/// Its input followed: at a steady speed, the whole range in `rise` (going up) or `fall` seconds
/// (glide), or slowing as it nears, a one-pole with those times (lag).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smooth {
    pub lag: bool,
    pub rise: f32,
    pub fall: f32,
}

impl Smooth {
    /// One frame; `state[0]` is where it is.
    pub fn step(&self, x: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
        let level = &mut state[0];
        let time = if x > *level { self.rise } else { self.fall };
        if time <= 0.0 {
            *level = x;
        } else if self.lag {
            *level += (x - *level) * (1.0 - (-dt / time).exp());
        } else {
            let most = dt / time;
            *level += (x - *level).clamp(-most, most);
        }
        *level
    }
}

/// Its input followed on a spring: a swing takes `time` seconds, and `bounce` (0 to under 1) is
/// how far it overshoots and how long it rings (0 settles without passing it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub time: f32,
    pub bounce: f32,
}

impl Spring {
    /// One frame; `state` is (where it is, how fast it moves).
    pub fn step(&self, x: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
        let w = std::f32::consts::TAU / self.time.max(0.01);
        let damping = 1.0 - self.bounce.clamp(0.0, 0.95);
        // steps small enough to stay steady however fast it swings or long the frame
        let steps = (w * dt / 0.2).ceil().clamp(1.0, 256.0);
        let h = dt / steps;
        let [at, speed, ..] = state;
        for _ in 0..steps as u32 {
            *speed += (w * w * (x - *at) - 2.0 * damping * w * *speed) * h;
            *at += *speed * h;
        }
        *at
    }
}

/// How fast its input changes: 1 when it crosses the whole range in `time` seconds, counting
/// the way [`SLOPE_DIRECTIONS`] says.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slope {
    pub direction: u8,
    pub time: f32,
}

impl Slope {
    /// One frame; `state[0]` is the last input (NaN before the first).
    pub fn step(&self, x: f32, dt: f32, state: &mut [f32; 4]) -> f32 {
        let last = std::mem::replace(&mut state[0], x);
        if last.is_nan() || dt <= 0.0 {
            return 0.0;
        }
        let rate = (x - last) / dt * self.time;
        (match self.direction {
            1 => -rate,
            2 => rate.abs(),
            _ => rate,
        })
        .clamp(0.0, 1.0)
    }
}

/// A picture read as a number: one of [`PROBE_MEASURES`] of its average color over a region,
/// `size` of the picture each way round `position` (-1 to 1; size 0 is a point).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub measure: u8,
    pub position: [f32; 2],
    pub size: f32,
}

impl Probe {
    /// Its value for an average color (premultiplied).
    pub fn measure(&self, c: [f32; 4]) -> f32 {
        (match self.measure {
            1 => c[0],
            2 => c[1],
            3 => c[2],
            4 => c[3],
            _ => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2],
        })
        .clamp(0.0, 1.0)
    }
}

/// What a signal node does, with its settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The audio features, as the shared sources have them.
    Audio,
    /// The beat's hit and the bar's ramp.
    Clock,
    Lfo(Lfo),
    /// Its cycle in seconds and its shape ([`TIME_SHAPES`]).
    Time(f32, u8),
    /// Its kick's shape, and how strong its input must be for any.
    BeatKick(Envelope, f32),
    Remap(Remap),
    Math(Math),
    /// How many times its input.
    Gain(f32),
    Envelope(Envelope),
    Counter(Counter),
    Sequencer(Sequencer),
    /// Its threshold.
    Hold(f32),
    Toggle(f32),
    /// Below (or above), and the value b is while nothing's wired into it.
    Compare(bool, f32),
    /// Its period in seconds.
    Timer(f32),
    Random(Random),
    Smooth(Smooth),
    /// How many steps.
    Quantize(u8),
    /// Seconds a cycle takes at full speed (its input 1, or nothing wired), and its shape
    /// ([`TIME_SHAPES`]).
    Speed(f32, u8),
    /// How many beats back.
    Delay(f32),
    Spring(Spring),
    Slope(Slope),
    Probe(Probe),
}

impl Kind {
    /// A signal node's kind and settings, from its node; none for anything else.
    pub fn of(node: &velo_core::Node) -> Option<Self> {
        let get = |p: &str| node.params.get(p).map(|v| v[0]);
        let v = |p: &str, d: f32| get(p).unwrap_or(d);
        let index = |p: &str, max: usize| v(p, 0.0).round().clamp(0.0, max as f32) as u8;
        Some(match node.kind.as_str() {
            AUDIO => Kind::Audio,
            CLOCK => Kind::Clock,
            // a sharp rise and a fall that drops fast, then tails off
            BEAT_KICK => Kind::BeatKick(Envelope { threshold: 0.5, attack: 0.005, release: v("release_ms", 250.0).clamp(20.0, 2000.0) / 1000.0, curve: 1 }, v("threshold", 0.3)),
            LFO => Kind::Lfo(Lfo::from_params(get)),
            TIME => Kind::Time(v("seconds", 10.0).clamp(0.5, 600.0), index("shape", TIME_SHAPES.len() - 1)),
            REMAP => Kind::Remap(Remap::from_params(get)),
            MATH => Kind::Math(Math { op: index("operation", MATH_OPS.len() - 1), value: v("value", 0.5) }),
            GAIN => Kind::Gain(v("gain", 2.0).clamp(0.0, GAIN_MAX)),
            ENVELOPE => Kind::Envelope(Envelope { threshold: v("threshold", 0.5), attack: v("attack_ms", 5.0) / 1000.0, release: v("release_ms", 300.0) / 1000.0, curve: index("curve", CURVES.len() - 1) }),
            COUNTER => Kind::Counter(Counter { steps: v("steps", 4.0).round().clamp(2.0, 16.0) as u8, mode: index("mode", COUNT_MODES.len() - 1), threshold: v("threshold", 0.5) }),
            SEQUENCER => Kind::Sequencer(Sequencer {
                steps: v("steps", 8.0).round().clamp(2.0, 16.0) as u8,
                beats: SEQ_BEATS[v("rate", 2.0).round().clamp(0.0, (SEQ_RATES.len() - 1) as f32) as usize],
                values: std::array::from_fn(|i| v(&format!("step_{}", i + 1), if i % 4 == 0 { 1.0 } else { 0.0 }).clamp(0.0, 1.0)),
            }),
            HOLD => Kind::Hold(v("threshold", 0.5)),
            TOGGLE => Kind::Toggle(v("threshold", 0.5)),
            COMPARE => Kind::Compare(index("operation", 1) == 1, v("value", 0.5)),
            TIMER => Kind::Timer(v("seconds", 4.0).clamp(0.1, 600.0)),
            RANDOM => Kind::Random(Random { beats: v("beats", 1.0).clamp(0.25, 64.0), glide: v("glide_ms", 1.0).max(0.0) / 1000.0, threshold: v("threshold", 0.5) }),
            SMOOTH => Kind::Smooth(Smooth { lag: index("mode", SMOOTH_MODES.len() - 1) == 1, rise: v("rise_ms", 300.0).max(0.0) / 1000.0, fall: v("fall_ms", 300.0).max(0.0) / 1000.0 }),
            QUANTIZE => Kind::Quantize(v("steps", 4.0).round().clamp(2.0, 32.0) as u8),
            SPEED => Kind::Speed(v("seconds", 4.0).clamp(0.1, 600.0), index("shape", TIME_SHAPES.len() - 1)),
            DELAY => Kind::Delay(v("beats", 1.0).clamp(0.25, DELAY_BEATS)),
            SPRING => Kind::Spring(Spring { time: v("time_ms", 500.0).clamp(50.0, 4000.0) / 1000.0, bounce: v("bounce", 0.5).clamp(0.0, 0.95) }),
            SLOPE => Kind::Slope(Slope { direction: index("direction", SLOPE_DIRECTIONS.len() - 1), time: v("time_ms", 100.0).clamp(10.0, 2000.0) / 1000.0 }),
            PROBE => {
                Kind::Probe(Probe { measure: index("measure", PROBE_MEASURES.len() - 1), position: node.params.get("position").map_or([0.0; 2], |v| [v[0].clamp(-1.0, 1.0), v[1].clamp(-1.0, 1.0)]), size: v("size", 1.0).clamp(0.0, 1.0) })
            }
            _ => return None,
        })
    }

    pub fn outputs(&self) -> usize {
        match self {
            Kind::Audio => 8,
            Kind::Clock => 2,
            _ => 1,
        }
    }
}

/// A signal node as the render thread runs it: what it is, where its inputs come from (slots),
/// where its outputs go, what it keeps between frames (a Delay its input's past too).
#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub kind: Kind,
    pub inputs: [Option<usize>; 2],
    pub slot: usize,
    state: [f32; 4],
    history: VecDeque<(f64, f32)>,
}

impl Signal {
    /// A Probe's color, as its picture side last read it.
    pub(crate) fn feed(&mut self, color: [f32; 4]) {
        self.state = color;
    }
}

/// A scene's signal nodes in the order they run (a node after what feeds it), and the name of
/// each output's slot ("n_ab12/0"), the slots counting on from the shared sources'.
pub fn plan(scene: &Scene) -> (Vec<Signal>, Vec<String>) {
    plan_from(scene, SOURCES.len())
}

/// The shared signal graph's plan: as a scene's, its slots from [`SHARED`] on.
pub fn plan_shared(graph: &Scene) -> (Vec<Signal>, Vec<String>) {
    plan_from(graph, SHARED)
}

/// What the signals a graph ran keep between frames handed to its new plan, node by node (by
/// each one's first output's name) where the node is still the same kind: a shared graph that
/// gains a node goes on without a Smooth starting over or a Beat Kick cut mid-fall.
pub fn carry(new: &mut [Signal], new_keys: &[String], old: &mut [Signal], old_keys: &[String]) {
    let first = |signals: &[Signal], keys: &[String], i: usize| keys.get(signals[i].slot - signals[0].slot).cloned();
    for i in 0..new.len() {
        let Some(key) = first(new, new_keys, i) else { continue };
        if let Some(j) = (0..old.len()).find(|&j| first(old, old_keys, j).as_ref() == Some(&key))
            && std::mem::discriminant(&old[j].kind) == std::mem::discriminant(&new[i].kind)
        {
            new[i].state = old[j].state;
            new[i].history = std::mem::take(&mut old[j].history);
        }
    }
}

fn plan_from(scene: &Scene, base: usize) -> (Vec<Signal>, Vec<String>) {
    let nodes: Vec<&velo_core::Node> = scene.nodes.iter().filter(|n| !n.off && node_kind(&n.kind).is_some_and(|k| k.category == Category::Signal)).collect();
    let feeds = |n: &velo_core::Node, port: u8| scene.wire_into(&n.id, port).map(|e| e.from.clone());
    let (mut order, mut placed): (Vec<&velo_core::Node>, Vec<bool>) = (Vec::new(), vec![false; nodes.len()]);
    // what's fed by nothing (or by what's placed) goes next; a loop never places
    let ready = |n: &velo_core::Node, order: &[&velo_core::Node]| (0..2).all(|p| feeds(n, p).is_none_or(|(from, _)| order.iter().any(|o| o.id == from) || !nodes.iter().any(|n| n.id == from)));
    while let Some(i) = (0..nodes.len()).find(|&i| !placed[i] && ready(nodes[i], &order)) {
        placed[i] = true;
        order.push(nodes[i]);
    }
    let (mut signals, mut keys) = (Vec::new(), Vec::<String>::new());
    for n in order {
        let Some(kind) = Kind::of(n) else { continue };
        if keys.len() + kind.outputs() > MAX_SIGNALS {
            break;
        }
        let slot = base + keys.len();
        let inputs = [0, 1].map(|p| feeds(n, p).and_then(|(from, port)| keys.iter().position(|k| *k == format!("{from}/{port}")).map(|i| base + i)));
        keys.extend((0..kind.outputs()).map(|p| format!("{}/{p}", n.id)));
        // a Remap's range unlearned (low over high), the last inputs high: no hit on the first
        // frame; a Random from the middle, picking at once on the clock; a Probe dark till it reads;
        // a Spring at rest at 0; a Slope with no last input, so no change on its first frame
        let state = match kind {
            Kind::Random(_) => [0.5, 0.5, 0.0, f32::NAN],
            Kind::Probe(_) | Kind::Spring(_) => [0.0; 4],
            Kind::Slope(_) => [f32::NAN, 0.0, 0.0, 0.0],
            _ => [0.0, 1.0, 0.0, 1.0],
        };
        signals.push(Signal { kind, inputs, slot, state, history: VecDeque::new() });
    }
    (signals, keys)
}

/// One frame: each signal node, in order, writes its outputs into `sources` (the shared ones
/// first: audio features, beat, bar).
pub fn run(signals: &mut [Signal], sources: &mut [f32], beats: f64, dt: f32) {
    for s in signals {
        let input = |p: usize| s.inputs[p].map_or(0.0, |i| sources[i]);
        match s.kind {
            Kind::Audio => {
                for k in 0..7 {
                    sources[s.slot + k] = sources[k];
                }
                sources[s.slot + 7] = sources[9];
            }
            Kind::Clock => {
                sources[s.slot] = sources[7];
                sources[s.slot + 1] = sources[8];
            }
            // the trigger's hits (the beat while nothing's wired) as strong as `in` (the bass),
            // none while that's under the threshold
            Kind::BeatKick(envelope, threshold) => {
                let strength = sources[s.inputs[0].unwrap_or(1)];
                let kick = envelope.step(sources[s.inputs[1].unwrap_or(7)], dt, &mut s.state);
                sources[s.slot] = if strength < threshold { 0.0 } else { kick * strength };
            }
            Kind::Lfo(lfo) => sources[s.slot] = lfo.at(beats),
            // where it is in its cycle, kept between frames
            Kind::Time(seconds, shape) => {
                s.state[0] = (s.state[0] + dt / seconds).fract();
                sources[s.slot] = cycle(shape, s.state[0]);
            }
            // as Time, its input the speed (full while nothing's wired); a hit on reset starts over
            Kind::Speed(seconds, shape) => {
                if hit(input(1), &mut s.state[1], 0.5) {
                    s.state[0] = 0.0;
                }
                let speed = s.inputs[0].map_or(1.0, |i| sources[i]);
                s.state[0] = (s.state[0] + speed * dt / seconds).rem_euclid(1.0);
                sources[s.slot] = cycle(shape, s.state[0]);
            }
            Kind::Delay(back) => sources[s.slot] = delay(input(0), beats, back, &mut s.history),
            Kind::Spring(spring) => sources[s.slot] = spring.step(input(0), dt, &mut s.state),
            Kind::Slope(slope) => sources[s.slot] = slope.step(input(0), dt, &mut s.state),
            Kind::Remap(remap) => sources[s.slot] = remap.step(input(0), dt, &mut s.state),
            Kind::Math(math) => sources[s.slot] = math.at(input(0), s.inputs[1].map_or(math.value, |i| sources[i])),
            // amplified, never past full: a mapping's amount already says how far 1 moves
            Kind::Gain(gain) => sources[s.slot] = (input(0) * gain).clamp(0.0, 1.0),
            Kind::Envelope(envelope) => sources[s.slot] = envelope.step(input(0), dt, &mut s.state),
            Kind::Counter(counter) => sources[s.slot] = counter.step(input(0), input(1), &mut s.state),
            Kind::Sequencer(seq) => sources[s.slot] = seq.at(beats),
            Kind::Hold(threshold) => sources[s.slot] = hold(s.inputs[0].map(|i| sources[i]), input(1), threshold, &mut s.state),
            Kind::Toggle(threshold) => {
                if hit(input(0), &mut s.state[1], threshold) {
                    s.state[0] = 1.0 - s.state[0];
                }
                sources[s.slot] = s.state[0];
            }
            Kind::Timer(seconds) => sources[s.slot] = timer(seconds, input(0), dt, &mut s.state),
            Kind::Random(random) => sources[s.slot] = random.step(s.inputs[0].map(|i| sources[i]), beats, dt, s.slot, &mut s.state),
            Kind::Smooth(smooth) => sources[s.slot] = smooth.step(input(0), dt, &mut s.state),
            Kind::Probe(probe) => sources[s.slot] = probe.measure(s.state),
            Kind::Quantize(steps) => {
                let last = f32::from(steps.max(2) - 1);
                sources[s.slot] = (input(0).clamp(0.0, 1.0) * last).round() / last;
            }
            Kind::Compare(below, value) => {
                let (a, b) = (input(0), s.inputs[1].map_or(value, |i| sources[i]));
                sources[s.slot] = f32::from(u8::from(if below { a < b } else { a > b }));
            }
        }
    }
}

/// New settings for a graph's signal nodes, kept in place (a Remap keeps what it learned).
pub fn retune(signals: &mut [Signal], kinds: &[Kind]) {
    for (s, k) in signals.iter_mut().zip(kinds) {
        s.kind = *k;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A signal node fed from sources 0 and 1 (or not), its output in source 2, as plan starts it.
    fn node(kind: Kind, wired: [bool; 2]) -> Signal {
        Signal { kind, inputs: [wired[0].then_some(0), wired[1].then_some(1)], slot: 2, state: [0.0, 1.0, 0.0, 1.0], history: VecDeque::new() }
    }

    /// Its output for each pair of inputs, a frame each, at `beats`.
    fn outputs(s: &mut Signal, frames: &[(f32, f32, f64)]) -> Vec<f32> {
        frames
            .iter()
            .map(|&(a, b, beats)| {
                let mut sources = [a, b, 0.0];
                run(std::slice::from_mut(s), &mut sources, beats, 1.0 / 60.0);
                sources[2]
            })
            .collect()
    }

    #[test]
    fn a_sequencer_plays_its_steps_on_the_beat_clock() {
        let mut values = [0.0; 16];
        values[..4].copy_from_slice(&[1.0, 0.5, 0.0, 0.25]);
        let mut s = node(Kind::Sequencer(Sequencer { steps: 4, beats: 1.0, values }), [false; 2]);
        let at = |b: f64| (0.0, 0.0, b);
        assert_eq!(outputs(&mut s, &[at(0.0), at(1.2), at(2.9), at(3.5), at(4.0), at(9.0)]), [1.0, 0.5, 0.0, 0.25, 1.0, 0.5], "four steps a beat each, again and again");
        s.kind = Kind::Sequencer(Sequencer { steps: 4, beats: 0.25, values });
        assert_eq!(outputs(&mut s, &[at(0.3), at(1.1)]), [0.5, 1.0], "a step a quarter beat");
        // the node's defaults: a hit on every fourth of eight steps, a beat each
        let defaults =
            velo_core::Node { id: "n".into(), kind: SEQUENCER.into(), name: String::new(), off: false, pos: [0.0; 2], params: Default::default(), mappings: Vec::new(), code: None, file: None, media: None, text: None, icon: String::new() };
        let Some(Kind::Sequencer(seq)) = Kind::of(&defaults) else { panic!("a sequencer") };
        assert_eq!((0..8).map(|b| seq.at(f64::from(b))).collect::<Vec<_>>(), [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn sample_and_hold_keeps_its_input_or_a_random_value_from_hit_to_hit() {
        let mut s = node(Kind::Hold(0.5), [true, true]);
        // in, trigger: held at each trigger's rise, whatever the input does between
        let out = outputs(&mut s, &[(0.3, 0.0, 0.0), (0.3, 1.0, 0.0), (0.8, 1.0, 0.0), (0.8, 0.0, 0.0), (0.9, 1.0, 0.0)]);
        assert_eq!(out, [0.0, 0.3, 0.3, 0.3, 0.9]);
        // nothing into in: a new random value on each hit, never close to the one before
        let mut random = node(Kind::Hold(0.5), [false, true]);
        let held: Vec<f32> = (0..20).flat_map(|_| outputs(&mut random, &[(0.0, 0.0, 0.0), (0.0, 1.0, 0.0)])).skip(1).step_by(2).collect();
        assert!(held.windows(2).all(|w| (w[0] - w[1]).abs() > 0.05 && (0.0..1.0).contains(&w[1])), "{held:?}");
    }

    #[test]
    fn a_timer_hits_each_time_its_seconds_pass_and_starts_over_on_reset() {
        // a frame is 1/60 s and the period half a second: a hit about every 30 frames, falling
        // back in between
        let hits = |s: &mut Signal, resets: &[usize]| {
            let out: Vec<f32> = (0..100).map(|i| outputs(s, &[(if resets.contains(&i) { 1.0 } else { 0.0 }, 0.0, 0.0)])[0]).collect();
            assert!(out[15] < 0.2, "{out:?}");
            (1..out.len()).filter(|&i| out[i] > out[i - 1] + 0.5).collect::<Vec<usize>>()
        };
        let at = hits(&mut node(Kind::Timer(0.5), [false, false]), &[]);
        assert!(at.len() == 3 && (29..=30).contains(&at[0]) && at.windows(2).all(|w| (29..=31).contains(&(w[1] - w[0]))), "{at:?}");
        // a reset just before the mark puts it off a whole period
        let at = hits(&mut node(Kind::Timer(0.5), [true, false]), &[20]);
        assert!((49..=51).contains(&at[0]), "{at:?}");
    }

    #[test]
    fn a_toggle_flips_on_each_hit_and_compare_says_above_or_below() {
        let mut t = node(Kind::Toggle(0.5), [true, false]);
        assert_eq!(outputs(&mut t, &[(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 0.0, 0.0), (0.7, 0.0, 0.0)]), [0.0, 1.0, 1.0, 1.0, 0.0]);
        let mut above = node(Kind::Compare(false, 0.7), [true, false]);
        assert_eq!(outputs(&mut above, &[(0.5, 0.0, 0.0), (0.8, 0.0, 0.0)]), [0.0, 1.0], "a against the value");
        let mut below = node(Kind::Compare(true, 0.7), [true, true]);
        assert_eq!(outputs(&mut below, &[(0.5, 0.4, 0.0), (0.5, 0.6, 0.0)]), [0.0, 1.0], "a against b, wired");
    }

    #[test]
    fn signal_nodes_run_in_order_into_their_slots() {
        use serde_json::json;
        let mut session = velo_core::Session::default();
        session.run("scene.add", json!({})).unwrap();
        // Remap first in the scene, fed by an Audio In's kick: it still runs after it
        for kind in [REMAP, AUDIO] {
            session.run("node.add", json!({ "kind": kind })).unwrap();
        }
        let ids: Vec<String> = session.active().unwrap().nodes.iter().map(|n| n.id.clone()).collect();
        session.run("edge.connect", json!({ "from": [ids[1], 4], "to": [ids[0], 0] })).unwrap();
        let (mut signals, keys) = plan(session.active().unwrap());
        assert_eq!(keys.len(), 9, "eight audio outputs, then the remap's");
        assert_eq!(keys[4], format!("{}/4", ids[1]));
        assert_eq!(keys[8], format!("{}/0", ids[0]));
        let mut sources = [0.0f32; SOURCES.len() + MAX_SIGNALS];
        (sources[4], sources[9]) = (0.8, 0.3); // the kick, the melody
        run(&mut signals, &mut sources, 0.0, 1.0 / 60.0);
        assert_eq!((sources[SOURCES.len() + 4], sources[SOURCES.len() + 7]), (0.8, 0.3), "the audio node's kick and melody outputs");
        assert!(sources[SOURCES.len() + 8] >= 0.0, "the remap ran on it");
    }

    #[test]
    fn off_signals_have_no_slots_inputs_or_mapping_routes() {
        use serde_json::json;
        let mut session = velo_core::Session::default();
        session.run("scene.add", json!({})).unwrap();
        for kind in [AUDIO, REMAP, velo_core::LFO, "velo.noise"] {
            session.run("node.add", json!({ "kind": kind })).unwrap();
        }
        let ids: Vec<String> = session.active().unwrap().nodes.iter().map(|n| n.id.clone()).collect();
        session.run("edge.connect", json!({ "from": [ids[0], 0], "to": [ids[1], 0] })).unwrap();
        session.run("mapping.add", json!({ "node": ids[3], "param": "scale", "source": ids[2], "amount": 0.5 })).unwrap();
        session.run("node.toggle", json!({ "nodes": [ids[0], ids[2]], "on": false })).unwrap();
        let scene = session.active().unwrap();
        let (signals, keys) = plan(scene);
        assert_eq!(keys, [format!("{}/0", ids[1])]);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].inputs, [None, None]);
        let shader = crate::ShaderNode::parse(include_str!("../shaders/noise.wgsl")).unwrap();
        let routes = crate::routes(&shader, &scene.nodes[3].mappings, &keys, &[]);
        assert!(routes.is_empty());
        let base = shader.values(|_| None);
        let mut modulator = crate::Modulator::new(&shader, &base, routes);
        modulator.step(&[1.0; SOURCES.len() + MAX_SIGNALS], 1.0);
        assert_eq!(modulator.values(), base);
    }

    #[test]
    fn a_shared_beat_kick_moves_two_scenes_alike_and_a_deleted_one_reads_zero() {
        use serde_json::json;
        let mut s = velo_core::Session::default();
        // two scenes, each a Kaleido whose zoom follows the shared Beat Kick, and a kick of their own
        let mut kaleidos = Vec::new();
        for _ in 0..2 {
            s.run("scene.add", json!({})).unwrap();
            s.run("node.add", json!({ "kind": "velo.kaleido" })).unwrap();
            s.run("node.add", json!({ "kind": BEAT_KICK })).unwrap();
            kaleidos.push(s.active().unwrap().clone());
        }
        s.run("node.add", json!({ "scene": velo_core::SHARED, "kind": velo_core::SMOOTH })).unwrap();
        s.run("node.add", json!({ "scene": velo_core::SHARED, "kind": BEAT_KICK })).unwrap();
        let kick = s.project.shared.nodes[1].id.clone();
        for scene in &s.project.scenes.clone() {
            s.run("mapping.add", json!({ "scene": scene.id, "node": scene.nodes[0].id, "param": "zoom", "source": format!("shared:{kick}") })).unwrap();
        }
        let shader = crate::ShaderNode::parse(include_str!("../shaders/kaleido.wgsl")).unwrap();
        let zoom = shader.params.iter().position(|p| p.id == "zoom").unwrap();
        let (mut shared, shared_keys) = plan_shared(&s.project.shared);
        assert_eq!((shared[1].slot, shared_keys[1].clone()), (SHARED + 1, format!("{kick}/0")), "after every scene's slots");
        let mut graphs: Vec<(Vec<Signal>, crate::Modulator)> = s
            .project
            .scenes
            .iter()
            .map(|scene| {
                let (signals, keys) = plan(scene);
                let routes = crate::routes(&shader, &scene.nodes[0].mappings, &keys, &shared_keys);
                assert_eq!(routes.len(), 1);
                (signals, crate::Modulator::new(&shader, &shader.values(|_| None), routes))
            })
            .collect();
        // a frame as the render thread runs it: the shared pass once, then each graph's own
        let frame = |shared: &mut Vec<Signal>, graphs: &mut Vec<(Vec<Signal>, crate::Modulator)>, beats: f64| {
            let mut sources = [0.0f32; SLOTS];
            (sources[1], sources[7]) = (1.0, if beats.fract() < 0.01 { 1.0 } else { 0.0 }); // the bass up, the beat's hit
            run(shared, &mut sources, beats, 1.0 / 60.0);
            graphs
                .iter_mut()
                .map(|(own, m)| {
                    run(own, &mut sources, beats, 1.0 / 60.0);
                    m.step(&sources, 1.0 / 60.0)[zoom * 16..zoom * 16 + 4].to_vec()
                })
                .collect::<Vec<_>>()
        };
        let mut seen = Vec::new();
        for k in 0..60 {
            let both = frame(&mut shared, &mut graphs, f64::from(k) / 30.0);
            assert_eq!(both[0], both[1], "frame {k}: both scenes read the same kick, though each ran a kick of its own");
            seen.push(f32::from_ne_bytes(both[0][..4].try_into().unwrap()));
        }
        assert!(seen.iter().any(|&z| z > 1.5) && seen.iter().any(|&z| z < 1.1), "it pulses: {seen:?}");
        // the shared graph gains a node in front: the kick keeps what it was doing, at its new slot
        let falling = shared[1].state;
        s.run("node.add", json!({ "scene": velo_core::SHARED, "kind": velo_core::LFO })).unwrap();
        s.project.shared.nodes.rotate_right(1);
        let (mut again, keys) = plan_shared(&s.project.shared);
        carry(&mut again, &keys, &mut shared, &shared_keys);
        let at = keys.iter().position(|k| *k == format!("{kick}/0")).unwrap();
        assert_eq!((at, again[at].state), (2, falling));
        // the kick deleted: its slot isn't written, mappings still pointing there read 0 and the
        // base comes back once its release is through; routed again, the mapping moves nothing
        s.run("node.remove", json!({ "scene": velo_core::SHARED, "node": kick })).unwrap();
        let (mut gone, gone_keys) = plan_shared(&s.project.shared);
        carry(&mut gone, &gone_keys, &mut again, &keys);
        for _ in 0..200 {
            frame(&mut gone, &mut graphs, 0.0);
        }
        let base = shader.values(|_| None)[zoom][0];
        assert!(graphs.iter().all(|(_, m)| (m.values()[zoom][0] - base).abs() < 1e-3), "back to the base");
        assert!(crate::routes(&shader, &s.project.scenes[0].nodes[0].mappings, &[], &gone_keys).is_empty());
    }

    #[test]
    fn a_remap_learns_its_range_and_shapes_it() {
        let remap = Remap { auto: true, input: [0.0, 1.0], output: [0.2, 1.0], curve: 0, gate: 0.0, attack: 0.0, release: 0.0, invert: false };
        let mut state = [0.0, 1.0, 0.0, 1.0];
        // a quiet signal between 0.1 and 0.3 comes out across the whole output range
        for x in [0.1, 0.3, 0.1, 0.3] {
            remap.step(x, 1.0 / 60.0, &mut state);
        }
        assert!((remap.step(0.3, 1.0 / 60.0, &mut state) - 1.0).abs() < 0.01);
        assert!((remap.step(0.1, 1.0 / 60.0, &mut state) - 0.2).abs() < 0.01);
        // by hand: the input range set, a gate, inverted
        let hand = Remap { auto: false, input: [0.0, 0.5], output: [0.0, 1.0], gate: 0.2, invert: true, ..remap };
        let mut state = [0.0, 1.0, 0.0, 1.0];
        assert!((hand.step(0.05, 0.016, &mut state) - 1.0).abs() < 1e-6, "under the gate: nothing, inverted");
        assert!(hand.step(0.5, 0.016, &mut state).abs() < 1e-6);
    }

    #[test]
    fn time_goes_round_its_cycle() {
        let mut signals = vec![Signal { slot: SOURCES.len(), ..node(Kind::Time(2.0, 0), [false; 2]) }];
        let mut sources = [0.0f32; SOURCES.len() + MAX_SIGNALS];
        let mut seen = Vec::new();
        for _ in 0..5 {
            run(&mut signals, &mut sources, 0.0, 0.5);
            seen.push(sources[SOURCES.len()]);
        }
        assert_eq!(seen, [0.25, 0.5, 0.75, 0.0, 0.25], "a ramp over 2 s, round again");
        signals[0].kind = Kind::Time(2.0, 1);
        run(&mut signals, &mut sources, 0.0, 0.5);
        assert_eq!(sources[SOURCES.len()], 1.0, "back and forth: at its top half way");
    }

    #[test]
    fn math_puts_two_signals_together_in_range() {
        let at = |op, a, b| Math { op, value: 0.0 }.at(a, b);
        let got = [at(0, 0.7, 0.6), at(1, 0.2, 0.5), at(2, 0.5, 0.5), at(3, 0.2, 0.9), at(4, 0.2, 0.9), at(5, 0.2, 0.6), at(6, 0.2, 0.6)];
        let want = [1.0, 0.0, 0.25, 0.2, 0.9, 0.4, 0.4];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-6), "{got:?}");
    }

    #[test]
    fn an_envelope_plays_on_each_hit() {
        let env = Envelope { threshold: 0.5, attack: 0.1, release: 0.2, curve: 0 };
        let mut state = [0.0, 1.0, 0.0, 1.0];
        let dt = 0.05;
        assert_eq!(env.step(0.9, dt, &mut state), 0.0, "high from the start: no hit");
        env.step(0.0, dt, &mut state);
        assert!((env.step(0.9, dt, &mut state) - 0.5).abs() < 1e-5, "half way up after half the attack");
        assert_eq!(env.step(0.9, dt, &mut state), 1.0, "at the top, held high: no new hit");
        assert!((env.step(0.0, dt, &mut state) - 0.75).abs() < 1e-5, "a quarter of the release");
        for _ in 0..3 {
            env.step(0.0, dt, &mut state);
        }
        assert_eq!(env.step(0.0, dt, &mut state), 0.0, "back down");
    }

    #[test]
    fn a_counter_steps_on_hits_and_resets() {
        let run = |mode, hits: usize| {
            let c = Counter { steps: 3, mode, threshold: 0.5 };
            let mut state = [0.0, 1.0, 0.0, 1.0];
            c.step(0.0, 0.0, &mut state);
            (0..hits).map(|_| (c.step(1.0, 0.0, &mut state), c.step(0.0, 0.0, &mut state)).0).collect::<Vec<f32>>()
        };
        assert_eq!(run(0, 4), [0.5, 1.0, 0.0, 0.5], "up");
        assert_eq!(run(1, 3), [0.5, 0.0, 1.0], "down");
        assert_eq!(run(2, 5), [0.5, 1.0, 0.5, 0.0, 0.5], "ping-pong");
        let random = run(3, 20);
        assert!(random.windows(2).all(|w| w[0] != w[1]) && random.iter().all(|v| [0.0, 0.5, 1.0].contains(v)), "{random:?}");
        let c = Counter { steps: 4, mode: 0, threshold: 0.5 };
        let mut state = [0.0, 1.0, 0.0, 1.0];
        for x in [0.0, 1.0, 0.0, 1.0, 0.0] {
            c.step(x, 0.0, &mut state);
        }
        assert_eq!(c.step(0.0, 0.0, &mut state), 2.0 / 3.0);
        assert_eq!(c.step(0.0, 1.0, &mut state), 0.0, "reset");
        assert_eq!(c.step(1.0, 1.0, &mut state), 1.0 / 3.0, "and on from there");
    }

    #[test]
    fn a_math_node_waits_for_both_its_inputs() {
        use serde_json::json;
        let mut session = velo_core::Session::default();
        session.run("scene.add", json!({})).unwrap();
        for kind in [MATH, AUDIO, CLOCK] {
            session.run("node.add", json!({ "kind": kind })).unwrap();
        }
        let ids: Vec<String> = session.active().unwrap().nodes.iter().map(|n| n.id.clone()).collect();
        session.run("edge.connect", json!({ "from": [ids[1], 4], "to": [ids[0], 0] })).unwrap();
        session.run("edge.connect", json!({ "from": [ids[2], 0], "to": [ids[0], 1] })).unwrap();
        session.run("node.set_param", json!({ "node": ids[0], "param": "operation", "value": 4.0 })).unwrap();
        let (mut signals, keys) = plan(session.active().unwrap());
        assert_eq!(keys.last(), Some(&format!("{}/0", ids[0])), "after the Audio In and the Beat");
        let mut sources = [0.0f32; SOURCES.len() + MAX_SIGNALS];
        (sources[4], sources[7]) = (0.3, 0.8);
        run(&mut signals, &mut sources, 0.0, 1.0 / 60.0);
        assert_eq!(sources[SOURCES.len() + keys.len() - 1], 0.8, "the max of the kick and the beat");
    }

    #[test]
    fn lfos_move_on_the_beat_grid() {
        let sine = Lfo { shape: 0, beats: 4.0, phase: 0.0 };
        assert!(sine.at(0.0).abs() < 1e-6 && (sine.at(2.0) - 1.0).abs() < 1e-6, "low on the bar, high half way");
        assert!((sine.at(4.0) - sine.at(0.0)).abs() < 1e-6, "a cycle every 4 beats");
        let saw = Lfo { shape: 2, beats: 1.0, phase: 0.25 };
        assert!((saw.at(0.5) - 0.75).abs() < 1e-6);
        let square = Lfo { shape: 3, beats: 2.0, phase: 0.0 };
        assert_eq!((square.at(0.5), square.at(1.5)), (1.0, 0.0));
        let random = Lfo { shape: 4, beats: 1.0, phase: 0.0 };
        assert_eq!(random.at(3.1), random.at(3.9), "held through the cycle");
        assert_ne!(random.at(3.1), random.at(4.1));
        assert!((0..100).map(|i| random.at(f64::from(i))).all(|v| (0.0..1.0).contains(&v)));
        let lfo = Lfo::from_params(|p| (p == "beats").then_some(100.0));
        assert_eq!((lfo.shape, lfo.beats), (0, 64.0), "clamped, defaults for the rest");
    }

    #[test]
    fn a_random_picks_on_the_beat_or_on_a_hit_and_glides() {
        let fresh = |kind| Signal { state: [0.5, 0.5, 0.0, f32::NAN], ..node(kind, [false; 2]) };
        // every two beats: one value through each step, another on the next, never close by
        let mut s = fresh(Kind::Random(Random { beats: 2.0, glide: 0.0, threshold: 0.5 }));
        let v = outputs(&mut s, &[(0.0, 0.0, 0.1), (0.0, 0.0, 1.9), (0.0, 0.0, 2.1), (0.0, 0.0, 3.9), (0.0, 0.0, 4.0)]);
        assert_eq!((v[0], v[2]), (v[1], v[3]), "held through a step: {v:?}");
        assert!((v[0] - v[2]).abs() >= 0.1 && (v[2] - v[4]).abs() >= 0.1 && v.iter().all(|x| (0.0..1.0).contains(x)), "{v:?}");
        // two Randoms go their own ways
        let mut other = Signal { slot: 3, ..fresh(Kind::Random(Random { beats: 2.0, glide: 0.0, threshold: 0.5 })) };
        let mut sources = [0.0; 4];
        run(std::slice::from_mut(&mut other), &mut sources, 0.1, 1.0 / 60.0);
        assert_ne!(sources[3], v[0]);
        // on a hit of its input instead, the clock left alone; gliding half a second there
        let mut s = Signal { inputs: [Some(0), None], ..fresh(Kind::Random(Random { beats: 1.0, glide: 0.5, threshold: 0.5 })) };
        let v = outputs(&mut s, &[(0.0, 0.0, 0.0), (0.0, 0.0, 5.0), (1.0, 0.0, 5.0)]);
        assert_eq!((v[0], v[1], v[2]), (0.5, 0.5, 0.5), "no hit yet, and a hit starts from where it is");
        let after = |frames: usize, s: &mut Signal| *outputs(s, &vec![(1.0, 0.0, 5.0); frames]).last().unwrap();
        let (quarter, end) = (after(15, &mut s), after(30, &mut s));
        assert!((quarter - 0.5).abs() > 0.0 && (quarter - 0.5).abs() < (end - 0.5).abs(), "on its way: {quarter} then {end}");
        assert_eq!(after(5, &mut s), end, "there after the glide");
    }

    #[test]
    fn a_probe_measures_the_color_it_was_fed() {
        use serde_json::json;
        let mut session = velo_core::Session::default();
        session.run("scene.add", json!({})).unwrap();
        session.run("node.add", json!({ "kind": PROBE })).unwrap();
        let (mut signals, keys) = plan(session.active().unwrap());
        assert_eq!((keys.len(), signals[0].kind), (1, Kind::Probe(Probe { measure: 0, position: [0.0; 2], size: 1.0 })), "the whole picture's brightness");
        let mut sources = [0.0f32; SOURCES.len() + MAX_SIGNALS];
        run(&mut signals, &mut sources, 0.0, 1.0 / 60.0);
        assert_eq!(sources[SOURCES.len()], 0.0, "dark till it reads");
        signals[0].feed([1.0, 0.5, 0.0, 1.0]);
        run(&mut signals, &mut sources, 0.0, 1.0 / 60.0);
        assert!((sources[SOURCES.len()] - (0.2126 + 0.7152 * 0.5)).abs() < 1e-6);
        let channel = |measure: u8| Probe { measure, position: [0.0; 2], size: 1.0 }.measure([0.25, 0.5, 0.75, 1.5]);
        assert_eq!([1, 2, 3, 4].map(channel), [0.25, 0.5, 0.75, 1.0], "a channel each, kept in 0..1");
    }

    #[test]
    fn a_smooth_glides_or_lags_and_a_quantize_snaps() {
        // glide: the whole range in a second up, half a second down
        let mut s = node(Kind::Smooth(Smooth { lag: false, rise: 1.0, fall: 0.5 }), [true, false]);
        let up = outputs(&mut s, &vec![(1.0, 0.0, 0.0); 30]);
        assert!((up[29] - 0.5).abs() < 1e-4, "half way up after half a second: {}", up[29]);
        let down = outputs(&mut s, &vec![(0.0, 0.0, 0.0); 15]);
        assert!(down[14].abs() < 1e-4, "and all the way down in a quarter: {}", down[14]);
        // lag: a one-pole, two thirds of the way after its time
        let mut s = node(Kind::Smooth(Smooth { lag: true, rise: 0.5, fall: 0.5 }), [true, false]);
        let v = outputs(&mut s, &vec![(1.0, 0.0, 0.0); 30]);
        assert!((v[29] - (1.0 - (-1.0f32).exp())).abs() < 1e-3, "{}", v[29]);
        let mut q = node(Kind::Quantize(4), [true, false]);
        let snapped = outputs(&mut q, &[(0.1, 0.0, 0.0), (0.2, 0.0, 0.0), (0.6, 0.0, 0.0), (0.95, 0.0, 0.0), (1.4, 0.0, 0.0)]);
        assert_eq!(snapped, [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0, 1.0]);
    }

    #[test]
    fn a_gain_multiplies_its_input_and_stays_within_0_and_1() {
        let scene_node = |gain: Option<f32>| velo_core::Node {
            id: "n".into(),
            kind: GAIN.into(),
            name: String::new(),
            off: false,
            pos: [0.0; 2],
            params: gain.map(|g| ("gain".to_string(), [g, 0.0, 0.0, 0.0])).into_iter().collect(),
            mappings: Vec::new(),
            code: None,
            file: None,
            media: None,
            text: None,
            icon: String::new(),
        };
        // twice by default; a gain from a file past its range is brought back into it
        assert_eq!(Kind::of(&scene_node(None)), Some(Kind::Gain(2.0)));
        assert_eq!(Kind::of(&scene_node(Some(40.0))), Some(Kind::Gain(GAIN_MAX)));
        assert_eq!(params(GAIN).unwrap()[0].default[0], 2.0);
        let mut g = node(Kind::Gain(4.0), [true, false]);
        assert_eq!(outputs(&mut g, &[(0.0, 0.0, 0.0), (0.1, 0.0, 0.0), (0.25, 0.0, 0.0), (0.6, 0.0, 0.0), (-0.5, 0.0, 0.0)]), [0.0, 0.4, 1.0, 1.0, 0.0]);
        // nothing wired in gives nothing
        assert_eq!(outputs(&mut node(Kind::Gain(16.0), [false, false]), &[(1.0, 0.0, 0.0)]), [0.0]);
    }

    #[test]
    fn a_speed_goes_round_as_fast_as_its_input_and_starts_over_on_reset() {
        // a turn in 2 s at full speed, a frame 1/60 s: nothing wired is full speed
        let mut s = node(Kind::Speed(2.0, 0), [false, false]);
        let v = outputs(&mut s, &vec![(0.0, 0.0, 0.0); 60]);
        assert!((v[59] - 0.5).abs() < 1e-4, "half a turn in a second: {}", v[59]);
        // wired: half speed, then standing still, never jumping back
        let mut s = node(Kind::Speed(2.0, 0), [true, true]);
        let half = outputs(&mut s, &vec![(0.5, 0.0, 0.0); 60]);
        assert!((half[59] - 0.25).abs() < 1e-4, "{}", half[59]);
        assert_eq!(outputs(&mut s, &[(0.0, 0.0, 0.0); 10]), [half[59]; 10], "no speed, no move");
        assert_eq!(outputs(&mut s, &[(0.0, 1.0, 0.0)]), [0.0], "reset");
        // round again after a whole turn, or back and forth
        let mut s = node(Kind::Speed(0.5, 1), [false, false]);
        let v = outputs(&mut s, &vec![(0.0, 0.0, 0.0); 45]);
        assert!((v[14] - 1.0).abs() < 1e-4 && v[29].abs() < 1e-4 && (v[44] - 1.0).abs() < 1e-4, "{v:?}");
    }

    #[test]
    fn a_delay_plays_its_input_back_some_beats_later() {
        // a beat back, the clock a quarter beat a frame: the input four frames later, 0 till then
        let mut s = node(Kind::Delay(1.0), [true, false]);
        let frames: Vec<(f32, f32, f64)> = (0..10).map(|i| (i as f32 / 10.0, 0.0, f64::from(i) * 0.25)).collect();
        let v = outputs(&mut s, &frames);
        assert_eq!(v[..4], [0.0; 4]);
        assert!(v[4..].iter().zip(&frames).all(|(v, f)| (v - f.0).abs() < 1e-6), "{v:?}");
        // longer while it plays: what it kept is there, up to DELAY_BEATS back
        s.kind = Kind::Delay(2.0);
        assert!((outputs(&mut s, &[(0.0, 0.0, 2.5)])[0] - 0.2).abs() < 1e-6, "two beats back: the third frame");
        // a clock standing still keeps one value; one going back starts over
        let still = s.history.len();
        outputs(&mut s, &vec![(0.7, 0.0, 2.5); 100]);
        assert_eq!(s.history.len(), still);
        assert_eq!(outputs(&mut s, &[(0.9, 0.0, 1.0)]), [0.0]);
        assert_eq!(s.history.len(), 1);
        // a long run keeps no more than DELAY_BEATS
        let mut s = node(Kind::Delay(16.0), [true, false]);
        outputs(&mut s, &(0..2000).map(|i| (1.0, 0.0, f64::from(i) * 0.05)).collect::<Vec<_>>());
        assert!(s.history.len() <= (f64::from(DELAY_BEATS) / 0.05) as usize + 1, "{}", s.history.len());
    }

    #[test]
    fn a_spring_overshoots_as_its_bounce_says_and_settles() {
        let run = |bounce: f32, dt: f32| {
            let spring = Spring { time: 0.5, bounce };
            let mut state = [0.0; 4];
            (0..(3.0 / dt) as usize).map(|_| spring.step(1.0, dt, &mut state)).collect::<Vec<f32>>()
        };
        let peak = |v: &[f32]| v.iter().copied().fold(0.0, f32::max);
        let (none, some, lots) = (run(0.0, 1.0 / 60.0), run(0.5, 1.0 / 60.0), run(0.9, 1.0 / 60.0));
        assert!(peak(&none) <= 1.0 + 1e-3, "no bounce, no overshoot: {}", peak(&none));
        assert!(peak(&some) > 1.05 && peak(&some) < peak(&lots), "{} then {}", peak(&some), peak(&lots));
        assert!((some.last().unwrap() - 1.0).abs() < 0.01, "settled after 3 s: {}", some.last().unwrap());
        // a long frame (a hitch) stays steady
        assert!(run(0.9, 0.5).iter().all(|v| v.is_finite() && v.abs() < 3.0));
    }

    #[test]
    fn a_slope_says_how_fast_its_input_rises_or_falls() {
        // the whole range in a tenth of a second is 1; a frame is 1/60 s
        let mut s = Signal { state: [f32::NAN, 0.0, 0.0, 0.0], ..node(Kind::Slope(Slope { direction: 0, time: 0.1 }), [true, false]) };
        let v = outputs(&mut s, &[(0.5, 0.0, 0.0), (0.55, 0.0, 0.0), (0.8, 0.0, 0.0), (0.7, 0.0, 0.0)]);
        assert_eq!(v[0], 0.0, "nothing to compare the first frame with");
        assert!((v[1] - 0.3).abs() < 1e-3 && v[2] == 1.0 && v[3] == 0.0, "{v:?}");
        s.kind = Kind::Slope(Slope { direction: 1, time: 0.1 });
        assert!((outputs(&mut s, &[(0.65, 0.0, 0.0)])[0] - 0.3).abs() < 1e-3, "falling counts");
        s.kind = Kind::Slope(Slope { direction: 2, time: 0.1 });
        assert!((outputs(&mut s, &[(0.7, 0.0, 0.0)])[0] - 0.3).abs() < 1e-3, "either way");
    }

    #[test]
    fn a_beat_kick_hits_on_the_beat_as_hard_as_the_bass_and_not_while_it_is_low() {
        let kick = |n: &velo_core::Node| Kind::of(n).unwrap();
        let defaults =
            velo_core::Node { id: "n".into(), kind: BEAT_KICK.into(), name: String::new(), off: false, pos: [0.0; 2], params: Default::default(), mappings: Vec::new(), code: None, file: None, media: None, text: None, icon: String::new() };
        // in (the bass), trigger (the beat): a beat with a loud bass, then a quiet one
        let mut s = node(kick(&defaults), [true, true]);
        let v = outputs(&mut s, &[(0.8, 0.0, 0.0), (0.8, 1.0, 0.0), (0.8, 0.9, 0.0), (0.8, 0.8, 0.0)]);
        assert_eq!(v[0], 0.0, "no beat yet");
        assert!(v[1] > 0.7 && v[1] <= 0.8 + 1e-6, "the kick as strong as the bass: {v:?}");
        assert!(v[3] < v[1], "falling: {v:?}");
        let quiet = outputs(&mut s, &[(0.8, 0.0, 0.0), (0.2, 1.0, 0.0), (0.2, 0.5, 0.0)]);
        assert_eq!(quiet[1..], [0.0, 0.0], "the bass under the threshold: no kick");
        // nothing wired: the shared bass and beat
        let mut s = Signal { slot: SOURCES.len(), ..node(kick(&defaults), [false, false]) };
        let mut sources = [0.0; SOURCES.len() + 1];
        let mut frame = |bass: f32, beat: f32| {
            (sources[1], sources[7]) = (bass, beat);
            run(std::slice::from_mut(&mut s), &mut sources, 0.0, 1.0 / 60.0);
            sources[SOURCES.len()]
        };
        assert_eq!(frame(0.9, 0.1), 0.0);
        assert!(frame(0.9, 1.0) > 0.8, "on the beat, the bass loud");
    }
}
