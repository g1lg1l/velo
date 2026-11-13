//! Feature extraction (docs/audio.md §3). Biquads split the signal into low, mid and high, and
//! every hop (256 samples, ~5 ms at 48 kHz) measures each band's level. The hits (kick, snare or
//! clap, hat) come one of two ways ([`Mode`]). Simple: a detector per hit watches its own band and
//! fires when the band's energy jumps past its threshold, in the hop the hit starts; every setting
//! is the user's. Model (the default): the drum model (drums.rs), a small network trained on
//! generated electronic drums and real electronic music (models/README.md), its probabilities
//! firing a hit as they rise past their thresholds, three hops later. The melody is the model's
//! too in Model mode (how much of the sound is tune, learned from separated melodies); in Simple
//! mode the short spectrum's peaks (spectrum.rs) stand in for it.

use std::f32::consts::{FRAC_1_SQRT_2, TAU};

use crate::drums::DrumNet;

/// Samples per analysis step.
pub const HOP: usize = 256;
/// Level bands, in [`Hop::bands_db`] order.
pub const BANDS: [&str; 3] = ["low", "mid", "high"];
/// Hits, in [`Hop::onsets`] order: a snare and a clap are one.
pub const HITS: [&str; 3] = ["kick", "snare", "hat"];

/// Below this a band is silent.
const SILENCE_DB: f32 = -70.0;
/// The drum model's weights (models/drums.bin; see models/README.md for how they were made) and
/// the probability each hit fires at unless the user moves it (tuned on held-out minutes).
const DRUM_WEIGHTS: &[u8] = include_bytes!("../models/drums.bin");
pub const MODEL_THRESHOLDS: [f32; 3] = [0.45, 0.35, 0.55];
/// Hops the model waits before it decides, ~16 ms: by then a kick's click has left its 21-ms
/// window and a clap's noise hasn't, so the two are told apart.
const LOOKAHEAD: u32 = 3;
/// After a hit, the model can't fire the same drum again for this long (s): one hit, one trigger.
const MODEL_REFRACTORY_S: [f32; 3] = [0.06, 0.06, 0.04];

/// Simple mode's longest energy window, in hops. A band under [`SLOW_UNDER_HZ`] is measured over
/// four hops (~21 ms): one 5-ms hop is shorter than a period of a 50-Hz note, and its energy
/// wobbles by ±3 dB from hop to hop.
const MAX_WINDOW: usize = 4;
const SLOW_UNDER_HZ: f32 = 400.0;
/// How close to its band's recent loudest (dB) a hit must get: in a techno mix the rolling bass
/// rises as sharply as the kick but stays ~8 dB under it; hats sit well under a snare's noise.
const NEAR_PEAK_DB: [f32; 3] = [3.0, 6.0, 12.0];
/// How fast that recent loudest forgets, per second.
const PEAK_DECAY_DB: f32 = 6.0;
/// A snare or a hat is still there at the end of the hop it starts in, with at least this share
/// of its loudest quarter hop; a kick's click (~2 ms, broadband) has gone.
const KEEP: f32 = 0.03;
/// Hops a detector waits after its band or the mode changed: its windows still hold the old sound.
const SETTLE: u32 = 3 * MAX_WINDOW as u32;
/// A hat this soon after a snare (s) is the snare's noise: a clap's bursts go on for ~30 ms and
/// reach the hats' band.
const SNARE_TAIL_S: f32 = 0.04;
/// A hat in a hop where a drum's body (130-400 Hz) rises this much (dB) is a kick's click: the
/// kick's own detector, on 21-ms windows, answers a hop or two later, but the top of its body is
/// there at once.
const BODY: Detect = Detect { threshold_db: 6.0, low_hz: 130.0, high_hz: 400.0, gap_s: 0.0 };

/// Where the hits come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// A detector per hit on its own band, every setting the user's: light, and nothing waits.
    Simple,
    /// The drum model: better on full mixes, ~4 % of a core, three hops later.
    #[default]
    Model,
}

/// One Simple-mode detector: a hit when its band rises `threshold_db` over its last windows,
/// no sooner than `gap_s` after the one before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detect {
    pub threshold_db: f32,
    pub low_hz: f32,
    pub high_hz: f32,
    pub gap_s: f32,
}

/// How the analyzer finds hits; changed while it runs with [`Analyzer::set`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub mode: Mode,
    /// Under this level (dBFS) nothing is a hit, in either mode.
    pub gate_db: f32,
    /// Simple mode's detectors, in [`HITS`] order.
    pub detect: [Detect; 3],
    /// Model mode's thresholds: how sure the model must be of each hit (0..1), in [`HITS`] order.
    pub model: [f32; 3],
}

impl Default for Settings {
    fn default() -> Self {
        let detect = |threshold_db, low_hz, high_hz, gap_s| Detect { threshold_db, low_hz, high_hz, gap_s };
        // tuned on a library of techno, hard techno, schranz, house and drum and bass (models/README.md):
        // the kick from 70 to 300 Hz, where its body sweeps down, over the rumble that fills the
        // lowest octave; snare and clap 1 to 8 kHz, rising 8 dB (a kick's click rises less); hats
        // over 8 kHz
        Self { mode: Mode::Model, gate_db: -60.0, detect: [detect(3.0, 70.0, 300.0, 0.1), detect(8.0, 1000.0, 8000.0, 0.08), detect(4.0, 8000.0, 20_000.0, 0.05)], model: MODEL_THRESHOLDS }
    }
}

impl Settings {
    /// Each hit's threshold, in [`Hop::signals`]' units: dB in Simple mode, a probability in Model mode.
    pub fn thresholds(&self) -> [f32; 3] {
        match self.mode {
            Mode::Simple => self.detect.map(|d| d.threshold_db),
            Mode::Model => self.model,
        }
    }
}

/// RBJ biquad, low- or high-pass (Ravune's analyze.rs).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}

impl Biquad {
    fn new(high: bool, freq: f32, rate: f32) -> Self {
        let w = TAU * (freq / rate).min(0.49);
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b = if high { [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0] } else { [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0] };
        Self { b: b.map(|v| v / a0), a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    /// Lets everything through (a band with fewer poles).
    fn through() -> Self {
        Self { b: [1.0, 0.0, 0.0], a: [0.0, 0.0], z: [0.0; 2] }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// One Simple-mode hit: its band, how its energy moved lately, when it last fired.
struct Detector {
    /// Two high-pass, two low-pass; an edge at the ends of the spectrum lets everything through.
    filters: [Biquad; 4],
    /// This hop's sum of squares, and its quarters'.
    sum: f32,
    quarters: [f32; 4],
    /// Mean-square energy of the last hops (a ring), and how many hops one measurement spans.
    energies: [f32; 3 * MAX_WINDOW],
    window: usize,
    hops: usize,
    /// The band's recent loudest hop (dB), decaying, and how close to it a hit must get.
    peak_db: f32,
    near_db: f32,
    threshold: f32,
    /// Hops since the last hit, and how many must pass before the next.
    since: u32,
    gap: u32,
    /// Whether a hit must still be there at its hop's end (snare, hat: a kick's click rises in
    /// their bands too, for a couple of milliseconds).
    sustained: bool,
    /// The last hop's rise (dB), what tempo follows; and the same when the hop could be a hit
    /// (over the gate, near the band's loudest, still there at its end), else 0: what the
    /// threshold is against, so crossing it is a hit unless the gap says no.
    rise: f32,
    signal: f32,
}

impl Detector {
    fn new(hit: usize, d: &Detect, rate: f32) -> Self {
        let mut detector = Self {
            filters: [Biquad::through(); 4],
            sum: 0.0,
            quarters: [0.0; 4],
            energies: [0.0; 3 * MAX_WINDOW],
            window: 1,
            hops: 0,
            peak_db: SILENCE_DB,
            near_db: NEAR_PEAK_DB[hit],
            threshold: 0.0,
            since: u32::MAX,
            gap: 0,
            sustained: hit > 0,
            rise: 0.0,
            signal: 0.0,
        };
        detector.tune(d, rate);
        detector
    }

    /// New settings; the filters keep their state, so nothing clicks.
    fn tune(&mut self, d: &Detect, rate: f32) {
        let hp = if d.low_hz > 20.0 { Biquad::new(true, d.low_hz, rate) } else { Biquad::through() };
        let lp = if d.high_hz < 0.45 * rate { Biquad::new(false, d.high_hz, rate) } else { Biquad::through() };
        for (f, new) in self.filters.iter_mut().zip([hp, hp, lp, lp]) {
            *f = Biquad { z: f.z, ..new };
        }
        self.window = if d.high_hz < SLOW_UNDER_HZ { MAX_WINDOW } else { 1 };
        self.gap = (d.gap_s * rate / HOP as f32).round() as u32;
        self.threshold = d.threshold_db;
    }

    fn run(&mut self, x: f32, quarter: usize) {
        let y = self.filters.iter_mut().fold(x, |y, f| f.run(y));
        self.sum += y * y;
        self.quarters[quarter] += y * y;
    }

    /// The hop's end: the rise it fired at, when it's a hit. `open`: the input is over the gate.
    fn step(&mut self, open: bool, peak_decay: f32) -> Option<f32> {
        let ring = 3 * MAX_WINDOW;
        self.energies[self.hops % ring] = self.sum / HOP as f32;
        self.hops += 1;
        let w = self.window;
        let mean = |back: usize| (0..w).map(|i| self.energies[(self.hops + ring * 2 - 1 - back - i) % ring]).sum::<f32>() / w as f32;
        let db = |e: f32| 10.0 * e.max(1e-12).log10();
        let db_now = db(mean(0));
        // the latest window against the louder of the two before it (silence counts as -70 dB:
        // a sound out of digital silence rises a hundred)
        self.rise = (db_now - db(mean(w)).max(db(mean(2 * w))).max(SILENCE_DB)).max(0.0);
        // how loud the band is now, for the peak: the newest two hops, already inside the hit,
        // not the whole window, still half made of what came before it
        let newest = (self.energies[(self.hops + ring - 1) % ring] + self.energies[(self.hops + ring - 2) % ring]) / 2.0;
        let level = db(newest).max(db_now);
        self.peak_db = (self.peak_db - peak_decay).max(level);
        self.since = self.since.saturating_add(1);
        let loudest = self.quarters.iter().copied().fold(0.0, f32::max);
        let kept = !self.sustained || self.quarters[3] >= loudest * KEEP;
        (self.sum, self.quarters) = (0.0, [0.0; 4]);
        let could = open && kept && level > self.peak_db - self.near_db && level > SILENCE_DB;
        self.signal = if could { self.rise } else { 0.0 };
        let hit = could && self.rise >= self.threshold && self.since > self.gap;
        if hit {
            self.since = 0;
        }
        hit.then_some(self.rise)
    }
}

/// One hop's features.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hop {
    /// Seconds since the analyzer started, at the end of this hop.
    pub time: f64,
    /// When the sound the hits (and `signals`, `strength`) describe ended: `time` in Simple mode,
    /// a couple of hops before it in Model mode, as the model waits to hear each attack whole.
    pub heard: f64,
    /// RMS level, dBFS.
    pub level_db: f32,
    /// Each band's RMS level, dBFS, in [`BANDS`] order.
    pub bands_db: [f32; 3],
    /// Hits this hop, in [`HITS`] order, with the signal they fired at.
    pub onsets: [Option<f32>; 3],
    /// What each hit's detector watched this hop, in [`HITS`] order and its threshold's units
    /// ([`Settings::thresholds`]): in Simple mode the band's rise (dB) when the hop could be a hit,
    /// else 0 (crossing the threshold is a hit unless it's too soon after the last); the model's
    /// probability in Model mode.
    pub signals: [f32; 3],
    /// What the beat tracker follows: the bands' rises (the kick's counting double), or the
    /// model's kick and snare probabilities (the hats sit between the beats).
    pub strength: f32,
    /// The tonal part's energy (dB): the peaks between 200 Hz and 4 kHz.
    pub tonal_db: f32,
    /// How much of the sound is melody (0..1), from the model: Model mode only.
    pub melody: Option<f32>,
}

/// Turns mono samples into [`Hop`]s. Allocates nothing after `new`.
pub struct Analyzer {
    rate: f32,
    settings: Settings,
    /// Settings waiting for the next hop to start.
    pending: Option<Settings>,
    filters: [[Biquad; 3]; 3],
    /// Sums of squares this hop: whole signal, then the bands.
    sums: [f32; 4],
    detectors: [Detector; 3],
    peak_decay: f32,
    /// Hops since the last snare, and how many after one a hat is its noise; a drum's body starting.
    since_snare: u32,
    snare_tail: u32,
    body: Detector,
    /// Hops left before a hit may fire again after the settings moved a band or the mode.
    settle: u32,
    drums: DrumNet,
    /// The model's last probabilities, hops since each drum last fired, and whether it can fire
    /// again (its probability has fallen back under half its threshold since).
    last: [f32; crate::drums::OUTPUTS],
    since: [u32; 3],
    armed: [bool; 3],
    refractory: [u32; 3],
    spectrum: crate::spectrum::Spectrum,
    /// The long spectrum: the low end, in bins fine enough to part a sub from a kick.
    long: crate::spectrum::Spectrum,
    filled: usize,
    hops: u64,
}

impl Analyzer {
    pub fn new(rate: f32) -> Self {
        Self::with(rate, Settings::default())
    }

    pub fn with(rate: f32, settings: Settings) -> Self {
        let lp = |f| Biquad::new(false, f, rate);
        let hp = |f| Biquad::new(true, f, rate);
        let hops_per_s = rate / HOP as f32;
        Self {
            rate,
            settings,
            pending: None,
            // bass under 150 Hz, mids 400 Hz to 5 kHz, highs over 6 kHz
            filters: [[lp(150.0), lp(150.0), Biquad::through()], [hp(400.0), hp(400.0), lp(5000.0)], [hp(6000.0), hp(6000.0), Biquad::through()]],
            sums: [0.0; 4],
            detectors: std::array::from_fn(|hit| Detector::new(hit, &settings.detect[hit], rate)),
            peak_decay: PEAK_DECAY_DB / hops_per_s,
            since_snare: u32::MAX,
            snare_tail: (SNARE_TAIL_S * hops_per_s).round() as u32,
            body: Detector::new(1, &BODY, rate),
            settle: 0,
            drums: DrumNet::new(rate, DRUM_WEIGHTS).expect("models/drums.bin matches the network"),
            last: [0.0; crate::drums::OUTPUTS],
            since: [u32::MAX; 3],
            armed: [true; 3],
            refractory: MODEL_REFRACTORY_S.map(|s| (s * hops_per_s).round() as u32),
            spectrum: crate::spectrum::Spectrum::new(rate),
            long: crate::spectrum::Spectrum::with_points(rate, crate::spectrum::LONG),
            filled: 0,
            hops: 0,
        }
    }

    /// The last hop's spectrum: its power per bin ([`crate::spectrum::Spectrum::power`]).
    pub fn spectrum(&self) -> &[f32] {
        self.spectrum.power()
    }

    /// The long spectrum ([`crate::spectrum::LONG`] points), as of this hop or the one before:
    /// the low end, finely.
    pub fn long_spectrum(&self) -> &[f32] {
        self.long.power()
    }

    pub fn rate(&self) -> f32 {
        self.rate
    }

    pub fn settings(&self) -> Settings {
        self.pending.unwrap_or(self.settings)
    }

    /// New settings, from the next hop on.
    pub fn set(&mut self, settings: Settings) {
        self.pending = (settings != self.settings).then_some(settings);
    }

    /// At a hop's start, so the drum model's hops stay lined up with these.
    fn apply(&mut self, s: Settings) {
        let band = |d: &Detect| (d.low_hz, d.high_hz);
        if s.mode != self.settings.mode || s.detect.iter().map(band).ne(self.settings.detect.iter().map(band)) {
            self.settle = SETTLE;
        }
        for (detector, d) in self.detectors.iter_mut().zip(&s.detect) {
            detector.tune(d, self.rate);
        }
        self.settings = s;
    }

    /// Feeds mono samples; calls `on_hop` for every hop they complete.
    pub fn push(&mut self, samples: &[f32], mut on_hop: impl FnMut(&Hop)) {
        for &x in samples {
            if self.filled == 0
                && let Some(s) = self.pending.take()
            {
                self.apply(s);
            }
            let model = self.settings.mode == Mode::Model;
            self.spectrum.push(x);
            self.long.push(x);
            // the model's hops line up with these: both count 256 from the first sample, and it
            // only stops or starts at a hop's start. Simple mode doesn't run it at all.
            let drums = if model { self.drums.push(x) } else { None };
            self.sums[0] += x * x;
            for (band, filters) in self.filters.iter_mut().enumerate() {
                let y = filters.iter_mut().fold(x, |y, f| f.run(y));
                self.sums[band + 1] += y * y;
            }
            if !model {
                let quarter = self.filled * 4 / HOP;
                for d in self.detectors.iter_mut().chain([&mut self.body]) {
                    d.run(x, quarter);
                }
            }
            self.filled += 1;
            if self.filled == HOP {
                let db = self.sums.map(|s| 10.0 * (s / HOP as f32).max(1e-12).log10());
                let reading = self.spectrum.hop();
                // every other hop (~11 ms) is plenty for a window eight times as long, at half the cost
                if self.hops.is_multiple_of(2) {
                    self.long.transform();
                }
                let open = db[0] > self.settings.gate_db && self.settle == 0;
                self.settle = self.settle.saturating_sub(1);
                self.hops += 1;
                let time = (self.hops * HOP as u64) as f64 / f64::from(self.rate);
                let (onsets, signals, strength, heard, melody) = if model {
                    let p = drums.unwrap_or(self.last);
                    let heard = time - f64::from(LOOKAHEAD) * HOP as f64 / f64::from(self.rate);
                    let hits = [p[0], p[1], p[2]];
                    (self.model_hits(p, &db, open), hits, p[0] + p[1], heard, Some(p[3]))
                } else {
                    let kick = self.detectors[0].step(open, self.peak_decay);
                    let snare = self.detectors[1].step(open, self.peak_decay);
                    // a hat in the same hop as a snare, or just after, is the snare's noise; in a hop
                    // where a drum's body rises, a kick's click (the price of not waiting a hop to
                    // see it gone: a hat right on a kick is lost)
                    self.since_snare = if snare.is_some() { 0 } else { self.since_snare.saturating_add(1) };
                    self.body.step(true, self.peak_decay);
                    let noise = self.since_snare <= self.snare_tail || self.body.rise >= BODY.threshold_db;
                    let hat = self.detectors[2].step(open && !noise, self.peak_decay);
                    let (onsets, signals) = ([kick, snare, hat], self.detectors.each_ref().map(|d| d.signal));
                    let rise = self.detectors.each_ref().map(|d| d.rise);
                    // the beat is where the kicks are, not the off-beat hats
                    let strength = if open { 2.0 * rise[0] + rise[1] + rise[2] } else { 0.0 };
                    (onsets, signals, strength, time, None)
                };
                on_hop(&Hop { time, heard, level_db: db[0], bands_db: [db[1], db[2], db[3]], onsets, signals, strength, tonal_db: reading.tonal_db, melody });
                (self.sums, self.filled) = ([0.0; 4], 0);
            }
        }
    }

    /// A drum fires as the model's probability rises past its threshold, once (a snare's tail
    /// wobbling around it isn't a second snare); never in a silent band: the model hears rises
    /// in dB, and a few dB of nothing rise too.
    fn model_hits(&mut self, p: [f32; crate::drums::OUTPUTS], db: &[f32; 4], open: bool) -> [Option<f32>; 3] {
        let mut onsets = [None; 3];
        for c in 0..3 {
            self.since[c] = self.since[c].saturating_add(1);
            if p[c] < self.settings.model[c] / 2.0 {
                self.armed[c] = true;
            }
            if open && p[c] >= self.settings.model[c] && self.armed[c] && self.since[c] > self.refractory[c] && db[c + 1] > SILENCE_DB {
                self.armed[c] = false;
                onsets[c] = Some(p[c]);
                self.since[c] = 0;
            }
        }
        self.last = p;
        onsets
    }
}

/// Onset times per hit (seconds), for a whole signal: the offline harness.
pub fn onsets(samples: &[f32], rate: f32, settings: Settings) -> [Vec<f64>; 3] {
    let mut found: [Vec<f64>; 3] = Default::default();
    Analyzer::with(rate, settings).push(samples, |hop| {
        for (band, onset) in hop.onsets.iter().enumerate() {
            if onset.is_some() {
                found[band].push(hop.time);
            }
        }
    });
    found
}

/// How well `found` matches `truth`: a hit counts when it comes no later than `window` seconds
/// after a true onset (and not before it: realtime can't see the future). Returns precision,
/// recall, F-measure and the average delay of the matched hits.
pub fn score(truth: &[f64], found: &[f64], window: f64) -> (f32, f32, f32, f64) {
    let (mut matched, mut delay, mut next) = (0usize, 0.0, 0usize);
    for &t in truth {
        while next < found.len() && found[next] < t - 0.001 {
            next += 1;
        }
        if next < found.len() && found[next] - t <= window {
            matched += 1;
            delay += found[next] - t;
            next += 1;
        }
    }
    let precision = matched as f32 / found.len().max(1) as f32;
    let recall = matched as f32 / truth.len().max(1) as f32;
    let f = if matched == 0 { 0.0 } else { 2.0 * precision * recall / (precision + recall) };
    (precision, recall, f, delay / matched.max(1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;

    /// Both modes with their defaults, how late each may answer on average, and how far under
    /// a test's F each may fall.
    fn modes() -> [(Settings, f64, f32); 2] {
        // the model waits three hops (~16 ms) to hear each attack whole. It learned on real
        // electronic music, where it beats Simple by 0.15 to 0.25 (models/README.md); on these
        // loops a hard click or a clap is now and then an extra hat or kick
        [(Settings { mode: Mode::Simple, ..Default::default() }, 0.012, 0.0), (Settings::default(), 0.026, 0.15)]
    }

    #[test]
    fn kicks_in_a_techno_loop_are_found_fast() {
        for (settings, late, slack) in modes() {
            for rate in [44_100.0, 48_000.0] {
                let take = synth::techno(rate, 128.0, 30.0, 7);
                let found = onsets(&take.samples, rate, settings);
                let (p, r, f, delay) = score(&take.kicks, &found[0], 0.03);
                assert!(f >= 0.9 - slack, "{:?}: kick F {f:.2} (precision {p:.2}, recall {r:.2}) at {rate} Hz", settings.mode);
                assert!(delay < late, "{:?}: kicks found {:.1} ms late on average", settings.mode, delay * 1000.0);
            }
        }
    }

    #[test]
    fn snares_and_hats_are_found_and_never_on_a_kick() {
        // the kick's click rises in their bands for a couple of milliseconds; a snare's noise
        // reaches the hats' band. This loop's kick clicks harder than most (2 ms of full-band
        // noise), and the model still takes a few for hats or snares: the measure that counts
        // is real music (models/README.md), this one catches a detector that got worse.
        for (settings, late, slack) in modes() {
            for rate in [44_100.0, 48_000.0] {
                let take = synth::techno(rate, 128.0, 30.0, 7);
                let found = onsets(&take.samples, rate, settings);
                for (name, truth, band) in [("snare", &take.snares, 1), ("hat", &take.hats, 2)] {
                    let (p, r, f, delay) = score(truth, &found[band], 0.03);
                    assert!(f >= 0.8 - slack, "{:?}: {name} F {f:.2} (precision {p:.2}, recall {r:.2}) at {rate} Hz", settings.mode);
                    assert!(delay < late, "{:?}: {name}s found {:.1} ms late on average", settings.mode, delay * 1000.0);
                }
            }
        }
    }

    #[test]
    fn a_clap_is_a_snare() {
        for (settings, _, slack) in modes() {
            let take = synth::claps(48_000.0, 124.0, 30.0, 11);
            let found = onsets(&take.samples, 48_000.0, settings);
            let (.., snare, _) = score(&take.snares, &found[1], 0.03);
            let (hp, hr, hat, _) = score(&take.hats, &found[2], 0.03);
            assert!(snare >= 0.8 - slack && hat >= 0.9 - slack, "{:?}: claps as snares F {snare:.2}, hats F {hat:.2} (precision {hp:.2}, recall {hr:.2})", settings.mode);
        }
    }

    #[test]
    fn quiet_input_is_as_good_as_loud() {
        // detection works on rises in dB, so the input level doesn't matter (no auto-gain needed)
        for (settings, ..) in modes() {
            let mut take = synth::techno(48_000.0, 124.0, 20.0, 3);
            take.samples.iter_mut().for_each(|s| *s *= 0.03); // -30 dB
            let (.., f, _) = score(&take.kicks, &onsets(&take.samples, 48_000.0, settings)[0], 0.03);
            assert!(f >= 0.9, "{:?}: kick F {f:.2} at -30 dB", settings.mode);
        }
    }

    #[test]
    fn silence_and_steady_tones_have_no_onsets() {
        let rate = 48_000.0;
        for (settings, ..) in modes() {
            let silence = vec![0.0; 48_000 * 5];
            assert!(onsets(&silence, rate, settings).iter().all(Vec::is_empty));
            let hum: Vec<f32> = (0..48_000 * 5).map(|i| 0.3 * (TAU * 55.0 * i as f32 / rate).sin()).collect();
            let found = onsets(&hum, rate, settings);
            // the tone starting is one onset at most per band; nothing after it
            assert!(found.iter().all(|band| band.iter().all(|&t| t < 0.2)), "{:?}: {found:?}", settings.mode);
        }
    }

    #[test]
    fn the_settings_are_the_users() {
        let rate = 48_000.0;
        let take = synth::techno(rate, 128.0, 20.0, 7);
        // after the first beat: the loop starting out of silence is a rise like no other
        let found = |settings: Settings| onsets(&take.samples, rate, settings)[0].clone();
        let kicks = |settings: Settings| found(settings).iter().filter(|&&t| t > 0.5).count();
        let mut s = Settings { mode: Mode::Simple, ..Default::default() };
        let normal = kicks(s);
        // a threshold no kick reaches, a gate over the loop's level
        s.detect[0].threshold_db = 60.0;
        assert_eq!(kicks(s), 0);
        s = Settings { gate_db: 0.0, ..Default::default() };
        assert_eq!(kicks(s), 0);
        // the kick's detector moved to the snare's band hears the snares
        s = Settings { mode: Mode::Simple, ..Default::default() };
        (s.detect[0].low_hz, s.detect[0].high_hz) = (400.0, 5000.0);
        let (_, recall, ..) = score(&take.snares, &found(s), 0.03);
        assert!(recall >= 0.9, "snares heard in the kick's detector: {recall:.2}");
        // a gap longer than a beat keeps one kick in two (128 BPM: a beat is 0.47 s)
        s = Settings { mode: Mode::Simple, ..Default::default() };
        s.detect[0].gap_s = 0.6;
        let halved = kicks(s);
        assert!(halved.abs_diff(normal / 2) <= 2, "{halved} of {normal} with a 0.6 s gap");
    }

    #[test]
    fn crossing_the_line_is_a_hit() {
        // what the graphs draw: in Simple mode a signal over the threshold fires, unless the
        // last hit was too recent
        let rate = 48_000.0;
        let take = synth::claps(rate, 124.0, 20.0, 11);
        let s = Settings { mode: Mode::Simple, ..Default::default() };
        let mut analyzer = Analyzer::with(rate, s);
        let (mut over, mut fired, mut last) = ([0usize; 3], [0usize; 3], [f64::MIN; 3]);
        analyzer.push(&take.samples, |hop| {
            for c in 0..3 {
                if hop.signals[c] >= s.detect[c].threshold_db && hop.time - last[c] > f64::from(s.detect[c].gap_s) {
                    over[c] += 1;
                }
                if hop.onsets[c].is_some() {
                    fired[c] += 1;
                    last[c] = hop.time;
                }
            }
        });
        assert_eq!(over, fired);
    }

    #[test]
    fn settings_change_while_it_listens() {
        // switching to the model halfway through: the second half is the model's, and as good
        let rate = 48_000.0;
        let take = synth::techno(rate, 128.0, 20.0, 7);
        let mut analyzer = Analyzer::with(rate, Settings { mode: Mode::Simple, ..Default::default() });
        let (half, mut found, mut signals) = (take.samples.len() / 2, Vec::new(), Vec::new());
        let mut on_hop = |hop: &Hop| {
            if hop.onsets[0].is_some() {
                found.push(hop.time);
            }
            signals.push(hop.signals[0]);
        };
        analyzer.push(&take.samples[..half + 100], &mut on_hop);
        analyzer.set(Settings { mode: Mode::Model, ..Default::default() });
        analyzer.push(&take.samples[half + 100..], &mut on_hop);
        let split = half as f64 / f64::from(rate) + 0.2;
        let truth: Vec<f64> = take.kicks.iter().copied().filter(|&t| t > split).collect();
        let after: Vec<f64> = found.iter().copied().filter(|&t| t > split).collect();
        let (.., f, _) = score(&truth, &after, 0.03);
        assert!(f >= 0.8, "kick F {f:.2} after switching to the model");
        // the model's signal is a probability
        assert!(signals[signals.len() - 200..].iter().all(|p| (0.0..=1.0).contains(p)));
    }

    /// Not a check: `cargo test --release -p velo-audio analysis_cost -- --ignored --nocapture`
    /// prints the share of a core each mode takes on a minute of the test loop.
    #[test]
    #[ignore]
    fn analysis_cost() {
        let rate = 48_000.0;
        let take = synth::techno(rate, 128.0, 60.0, 7);
        for mode in [Mode::Simple, Mode::Model] {
            let mut analyzer = Analyzer::with(rate, Settings { mode, ..Default::default() });
            let start = std::time::Instant::now();
            analyzer.push(&take.samples, |_| {});
            println!("{mode:?}: {:.2} % of a core", 100.0 * start.elapsed().as_secs_f64() / 60.0);
        }
        // the long spectrum's share of that, every other hop
        let mut long = crate::spectrum::Spectrum::with_points(rate, crate::spectrum::LONG);
        let start = std::time::Instant::now();
        for _ in 0..60 * 48_000 / (2 * HOP) {
            long.transform();
        }
        println!("long spectrum: {:.2} % of a core", 100.0 * start.elapsed().as_secs_f64() / 60.0);
    }

    #[test]
    fn scoring_counts_late_and_missing_hits() {
        let truth = [1.0, 2.0, 3.0, 4.0];
        let (p, r, f, delay) = score(&truth, &[1.005, 2.01, 3.5, 3.9], 0.03);
        assert_eq!((p, r), (0.5, 0.5));
        assert!((f - 0.5).abs() < 1e-6 && (delay - 0.0075).abs() < 1e-9);
    }
}
