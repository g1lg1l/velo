//! Auto-BPM (docs/audio.md §4), firming up as the music goes on. Every half second: the
//! autocorrelation of the last 10 seconds' onset strength scores each tempo from 70 to 180 BPM
//! (with its half, double and four-fold, leaning gently to ~125 against octave errors); a
//! posterior over tempo remembers those scores, forgetting slowly and blurred a little (tempo
//! drifts), so a steady track's tempo gets surer by the second and one odd window can't move it.
//! The beat: the comb of that period that lines up the most onset strength over the window; it
//! pulls the tracked phase halfway, unless it disagrees twice running (a real change). Allocates
//! nothing after `new`.

use crate::HOP;

/// The window it listens to, how often it estimates, and how much it needs before the first.
const WINDOW_S: f32 = 10.0;
const EVERY_S: f32 = 0.5;
const FIRST_S: f32 = 3.0;
pub const MIN_BPM: f32 = 70.0;
pub const MAX_BPM: f32 = 180.0;
/// The tempo grid's step (BPM).
const STEP: f32 = 0.25;
const TEMPI: usize = ((MAX_BPM - MIN_BPM) / STEP) as usize + 1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    pub bpm: f32,
    /// 0..1: how much of the belief sits within a BPM of this tempo (a steady track climbs
    /// toward 1 over a few seconds; noise or a breakdown stays low).
    pub confidence: f32,
    /// A beat's time, in the analyzer's seconds (the newest one before the estimate).
    pub beat_at: f64,
}

pub struct Tempo {
    hops_per_s: f32,
    envelope: Vec<f32>,
    at: usize,
    filled: usize,
    since: usize,
    /// Oldest-first copy of the ring (and its running sums), the autocorrelation by lag, the
    /// belief over tempo.
    line: Vec<f32>,
    sums: Vec<f64>,
    ac: Vec<f32>,
    belief: Vec<f32>,
    scratch: Vec<f32>,
    /// The tracked beat (seconds) and how many estimates running disagreed with it.
    phase: Option<f64>,
    disagree: u8,
}

impl Tempo {
    /// For an analyzer at `rate` samples a second.
    pub fn new(rate: f32) -> Self {
        let hops_per_s = rate / HOP as f32;
        let n = (WINDOW_S * hops_per_s) as usize;
        Self { hops_per_s, envelope: vec![0.0; n], at: 0, filled: 0, since: 0, line: vec![0.0; n], sums: vec![0.0; n + 1], ac: vec![0.0; n / 2], belief: vec![1.0 / TEMPI as f32; TEMPI], scratch: vec![0.0; TEMPI], phase: None, disagree: 0 }
    }

    /// One hop's onset strength at `time` (seconds); every half second, once it has heard a few
    /// seconds, a new estimate.
    pub fn push(&mut self, strength: f32, time: f64) -> Option<Estimate> {
        let n = self.envelope.len();
        self.envelope[self.at] = strength;
        self.at = (self.at + 1) % n;
        self.filled = (self.filled + 1).min(n);
        self.since += 1;
        if (self.filled as f32) < FIRST_S * self.hops_per_s || (self.since as f32) < EVERY_S * self.hops_per_s {
            return None;
        }
        self.since = 0;
        Some(self.estimate(time))
    }

    fn estimate(&mut self, time: f64) -> Estimate {
        let (n, len) = (self.envelope.len(), self.filled);
        // what it heard, oldest first, less its mean over the second around: a swell, or the
        // analysis settling at the start, isn't a pulse (and a trend correlates at every lag)
        for i in 0..len {
            self.line[i] = self.envelope[(self.at + n - len + i) % n];
            self.sums[i + 1] = self.sums[i] + f64::from(self.line[i]);
        }
        let w = (0.5 * self.hops_per_s) as usize;
        for i in 0..len {
            let (lo, hi) = (i.saturating_sub(w), (i + w + 1).min(len));
            self.line[i] -= ((self.sums[hi] - self.sums[lo]) / (hi - lo) as f64) as f32;
        }
        let line = &self.line[..len];
        let mean = line.iter().sum::<f32>() / len as f32;
        let lags = self.ac.len().min(len / 2);
        for lag in 0..lags {
            self.ac[lag] = line[..len - lag].iter().zip(&line[lag..]).map(|(a, b)| (a - mean) * (b - mean)).sum::<f32>() / (len - lag) as f32;
        }
        let r0 = self.ac[0].max(1e-9);
        let ac = &self.ac[..lags];
        let at = |lag: f32| {
            let i = lag as usize;
            if i + 1 >= ac.len() {
                return 0.0;
            }
            let f = lag - i as f32;
            (ac[i] * (1.0 - f) + ac[i + 1] * f) / r0
        };
        let hps = self.hops_per_s;
        // each tempo's evidence, then the belief: kept, blurred, sharpened by it
        // as strong as the pulse really is: probabilities that barely move (noise) barely move
        // the belief, a clear beat does
        let clear = (r0.sqrt() / 0.1).min(1.0);
        for (k, s) in self.scratch.iter_mut().enumerate() {
            let bpm = MIN_BPM + k as f32 * STEP;
            let lag = 60.0 * hps / bpm;
            let prior = (-0.5 * ((bpm / 125.0).log2() / 0.6).powi(2)).exp();
            *s = ((at(lag) + 0.5 * at(2.0 * lag) + 0.25 * at(lag / 2.0) + 0.25 * at(4.0 * lag)) * prior * clear).max(0.0);
        }
        let mut total = 0.0;
        let blurred: [f32; TEMPI] = std::array::from_fn(|k| (-8i32..=8).map(|d| (k as i32 + d).clamp(0, TEMPI as i32 - 1) as usize).zip(-8i32..=8).map(|(j, d)| self.belief[j] * (-0.5 * (d as f32 / 3.0).powi(2)).exp()).sum::<f32>());
        for (b, (blur, s)) in self.belief.iter_mut().zip(blurred.iter().zip(&self.scratch)) {
            *b = blur.powf(0.9) * (2.5 * s).exp();
            total += *b;
        }
        self.belief.iter_mut().for_each(|b| *b /= total.max(1e-30));
        let peak = (0..TEMPI).max_by(|&a, &b| self.belief[a].total_cmp(&self.belief[b])).unwrap_or(0);
        let confidence = self.belief[peak.saturating_sub(4)..(peak + 5).min(TEMPI)].iter().sum::<f32>();
        // between grid steps: the period whose multiples line up best across the window
        let comb = |lag: f32| (1..).map(|k| k as f32 * lag).take_while(|&l| l < lags as f32 - 1.0).map(at).sum::<f32>();
        let coarse = 60.0 * hps / (MIN_BPM + peak as f32 * STEP);
        let period = (-10..=10).map(|d| coarse * (1.0 + d as f32 * 0.0005)).max_by(|a, b| comb(*a).total_cmp(&comb(*b))).unwrap_or(coarse);
        // tracks sit on a whole or half tempo: 134.02 is 134, 134.34 is 134.5
        let bpm = (2.0 * 60.0 * hps / period).round() / 2.0;
        // the phase: the comb of that period, back from now, that lines up the most onset strength
        let line = &self.line[..len];
        let at_back = |back: f32| {
            let i = back as usize;
            if i + 1 >= len {
                return 0.0;
            }
            let f = back - i as f32;
            line[len - 1 - i] * (1.0 - f) + line[len - 2 - i] * f
        };
        let comb_at = |offset: usize| (0..).map(|k| k as f32 * period + offset as f32).take_while(|&b| (b as usize) + 1 < len).map(at_back).sum::<f32>();
        let offset = (0..period as usize).max_by(|&a, &b| comb_at(a).total_cmp(&comb_at(b))).unwrap_or(0);
        let found = time - offset as f64 / f64::from(hps);
        Estimate { bpm, confidence, beat_at: self.follow(found, f64::from(60.0 / bpm), time) }
    }

    /// The tracked beat, pulled halfway to the one just found; a jump only when two estimates
    /// running put it elsewhere. Returns the newest tracked beat before `now`.
    fn follow(&mut self, found: f64, period: f64, now: f64) -> f64 {
        let phase = match self.phase {
            None => found,
            Some(p) => {
                let err = (found - p) / period;
                let err = (err - err.round()) * period;
                if err.abs() < 0.25 * period {
                    self.disagree = 0;
                    p + 0.5 * err
                } else {
                    self.disagree += 1;
                    if self.disagree >= 2 { found } else { p }
                }
            }
        };
        let newest = phase + ((now - phase) / period).floor() * period;
        self.phase = Some(newest);
        newest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Analyzer, Mode, Settings, synth};

    const MODES: [Mode; 2] = [Mode::Simple, Mode::Model];

    /// The last estimate for a take.
    fn listen(samples: &[f32], rate: f32, mode: Mode) -> Estimate {
        let (mut analyzer, mut tempo, mut last) = (Analyzer::with(rate, Settings { mode, ..Default::default() }), Tempo::new(rate), None);
        analyzer.push(samples, |hop| {
            if let Some(e) = tempo.push(hop.strength, hop.heard) {
                last = Some(e);
            }
        });
        last.expect("a full window")
    }

    #[test]
    fn hears_the_tempo_of_four_on_the_floor_and_where_the_beat_is() {
        for (mode, bpm) in MODES.into_iter().flat_map(|m| [96.0, 120.0, 128.0, 140.0, 165.0].map(|b| (m, b))) {
            let take = synth::techno(48_000.0, bpm, 12.0, 3);
            let e = listen(&take.samples, 48_000.0, mode);
            assert!((e.bpm - bpm).abs() < 0.2, "{mode:?} {bpm}: heard {}", e.bpm);
            assert!(e.confidence > 0.25, "{mode:?} {bpm}: sure {}", e.confidence);
            // the beat it names is a kick, within 25 ms
            let off = take.kicks.iter().map(|k| (k - e.beat_at).abs()).fold(f64::MAX, f64::min);
            assert!(off < 0.025, "{mode:?} {bpm}: beat {:.3} s from the nearest kick", off);
        }
    }

    #[test]
    fn noise_is_not_sure_of_anything() {
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..48_000 * 10)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                (seed as f32 / u32::MAX as f32 - 0.5) * 0.5
            })
            .collect();
        for mode in MODES {
            let e = listen(&noise, 48_000.0, mode);
            assert!(e.confidence < 0.15, "{mode:?} noise: sure {}", e.confidence);
        }
    }
}
