//! The gesture looper: a macro's move over 1, 2 or 4 bars, recorded on the beat clock and
//! played back by itself until the macro is touched again. A performance state, kept in memory
//! only: it is never saved with the project and never goes through undo.

/// Beats in a bar, as the beat clock counts them (Link's 4-beat bar).
pub const BAR: f64 = 4.0;
/// Samples of the curve a beat: finer than a UI frame at any tempo a set plays.
const STEPS: f64 = 48.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Waiting for the next bar to start recording.
    Armed,
    Recording,
    Playing,
}

pub struct Loop {
    pub bars: u8,
    pub phase: Phase,
    /// The beat recording starts on, always on a bar.
    start: f64,
    curve: Vec<f32>,
    /// Samples recorded so far.
    written: usize,
    /// The document's value when playback started: any other means a hand took over.
    seen: f32,
    /// The value it gave last frame: what the engine and the knob show.
    pub shown: f32,
}

impl Loop {
    /// Armed at `beats`: it records from the next bar on.
    pub fn new(bars: u8, beats: f64, value: f32) -> Self {
        let samples = (f64::from(bars) * BAR * STEPS) as usize;
        Self { bars, phase: Phase::Armed, start: next_bar(beats), curve: vec![value; samples], written: 0, seen: value, shown: value }
    }

    fn len(&self) -> f64 {
        f64::from(self.bars) * BAR
    }

    /// One frame: `value` is the macro in the document (whatever moved it: the mouse, MIDI, a
    /// key), `beats` the beat clock. False once the macro was touched while it played.
    pub fn step(&mut self, value: f32, beats: f64) -> bool {
        if self.phase == Phase::Armed {
            // the clock went back (a resync): wait for its next bar instead
            if self.start - beats > BAR {
                self.start = next_bar(beats);
            }
            if beats >= self.start {
                self.phase = Phase::Recording;
            }
        }
        if self.phase == Phase::Recording {
            let end = ((beats - self.start).max(0.0) * STEPS) as usize + 1;
            let end = end.min(self.curve.len());
            self.curve[self.written.min(end)..end].fill(value);
            self.written = self.written.max(end);
            if beats >= self.start + self.len() {
                self.curve[self.written..].fill(value);
                (self.phase, self.seen) = (Phase::Playing, value);
            }
        }
        self.shown = match self.phase {
            Phase::Playing if value != self.seen => return false,
            Phase::Playing => self.at(beats),
            _ => value,
        };
        true
    }

    /// The recorded curve at `beats`: bar-locked, so it follows the tempo, not the seconds.
    fn at(&self, beats: f64) -> f32 {
        let x = (beats - self.start).rem_euclid(self.len()) * STEPS;
        let i = x as usize % self.curve.len();
        let next = self.curve[(i + 1) % self.curve.len()];
        self.curve[i] + (next - self.curve[i]) * x.fract() as f32
    }

    /// Where it is in its bars (0..1) while it records or plays.
    pub fn progress(&self, beats: f64) -> f32 {
        ((beats - self.start).rem_euclid(self.len()) / self.len()) as f32
    }
}

/// The first bar at or after `beats`.
fn next_bar(beats: f64) -> f64 {
    (beats / BAR).ceil() * BAR
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A loop of `bars` armed at beat 1.5 that records `curve(beat)` frame by frame at 60 fps
    /// and 120 BPM, then returns with playback started.
    fn recorded(bars: u8, curve: impl Fn(f64) -> f32) -> (Loop, f64) {
        let mut l = Loop::new(bars, 1.5, 0.0);
        let mut beats = 1.5;
        while l.phase != Phase::Playing {
            assert!(l.step(curve(beats), beats));
            beats += 2.0 / 60.0;
        }
        (l, beats)
    }

    #[test]
    fn recording_starts_on_the_next_bar() {
        let mut l = Loop::new(1, 1.5, 0.0);
        assert!(l.step(0.3, 3.9));
        assert_eq!(l.phase, Phase::Armed, "beat 3.9 is still the bar it was armed in");
        assert!(l.step(0.3, 4.01));
        assert_eq!(l.phase, Phase::Recording);
        assert_eq!(Loop::new(2, 8.0, 0.0).start, 8.0, "on a bar: that one");
    }

    #[test]
    fn playback_gives_what_was_recorded_at_that_beat_and_wraps() {
        // a ramp over the bar, 0 on its downbeat to 1 at its end
        let (mut l, _) = recorded(1, |b| ((b - 4.0) / 4.0).clamp(0.0, 1.0) as f32);
        let last = l.seen;
        for beat in [9.0, 10.0, 11.5, 13.0, 30.0] {
            assert!(l.step(last, beat));
            let want = ((beat - 8.0) % 4.0 / 4.0) as f32;
            assert!((l.shown - want).abs() < 0.02, "beat {beat}: {} wanted {want}", l.shown);
        }
        assert!(l.step(last, 16.0) && l.shown < 0.02, "the downbeat again: back to the start");
    }

    #[test]
    fn touching_it_stops_it() {
        let (mut l, beats) = recorded(2, |b| (b * 0.1).fract() as f32);
        let seen = l.seen;
        assert!(l.step(seen, beats));
        assert!(!l.step(seen + 0.1, beats + 0.1), "the macro moved by hand");
    }

    #[test]
    fn a_new_tempo_keeps_it_on_the_bar() {
        // recorded at 120 BPM, played at 90: every frame shows what the same beat of the bar had
        let (mut l, mut beats) = recorded(4, |b| ((b - 4.0) / 16.0).rem_euclid(1.0) as f32);
        let v = l.seen;
        for _ in 0..2000 {
            assert!(l.step(v, beats));
            let want = ((beats - 4.0) / 16.0).rem_euclid(1.0) as f32;
            // the ramp's drop back to 0 is the one place the curve jumps
            if want > 0.01 && want < 0.99 {
                assert!((l.shown - want).abs() < 0.01, "beat {beats:.2}: {} wanted {want}", l.shown);
            }
            assert!((l.progress(beats) - want).abs() < 1e-4);
            beats += 1.5 / 60.0;
        }
    }
}
