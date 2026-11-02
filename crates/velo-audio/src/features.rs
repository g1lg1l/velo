//! What shaders and mappings read (docs/audio.md §3): levels normalized 0..1 by auto-gain, hit
//! envelopes that jump to 1 on an onset and fall back, and audio-driven clocks that run faster
//! when a band is busy (motion that pulses with the music without jitter).

use crate::analysis::Hop;

/// Below this the input is silent: features fall to 0 instead of auto-gain lifting the hiss.
const SILENCE_DB: f32 = -60.0;
/// How fast the auto-gain's reference peak comes down, per second: ~10 s to forget 20 dB.
const PEAK_RELEASE_DB: f32 = 2.0;
/// A band's reference never falls further than this under the whole mix's: a band that goes
/// quiet (the bass in a breakdown) reads as quiet, instead of auto-gain lifting what's left.
const BAND_RANGE_DB: f32 = 24.0;
/// Level smoothing: quick up, slower down.
const ATTACK_S: f32 = 0.005;
const RELEASE_S: f32 = 0.15;
/// How long a hit takes to fall to 1/e.
const HIT_DECAY_S: f32 = 0.15;
/// The melody's smoothing: slower than a band's, a line rather than a pulse.
const MELODY_ATTACK_S: f32 = 0.03;
const MELODY_RELEASE_S: f32 = 0.4;

/// One moment of the music, every value 0..1 except the clocks (seconds, ever growing).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Features {
    pub level: f32,
    pub bass: f32,
    pub mid: f32,
    pub high: f32,
    pub kick: f32,
    pub snare: f32,
    pub hat: f32,
    /// How much tune there is: the tonal part (the spectrum's peaks, 200 Hz to 4 kHz), the
    /// melody and chords over the drums.
    pub melody: f32,
    pub level_time: f32,
    pub bass_time: f32,
    pub mid_time: f32,
    pub high_time: f32,
}

impl Features {
    /// In the order of the shader globals after `bar_phase` (velo_engine::GLOBALS).
    pub fn globals(&self) -> [f32; 12] {
        [self.level, self.bass, self.mid, self.high, self.kick, self.snare, self.hat, self.bass_time, self.mid_time, self.high_time, self.level_time, self.melody]
    }
}

/// Turns analysis hops into [`Features`].
#[derive(Default)]
pub struct Tracker {
    /// Auto-gain reference per level (whole, bass, mid, high, melody), dB.
    peaks: [f32; 5],
    smooth: [f32; 5],
    features: Features,
}

impl Tracker {
    pub fn new() -> Self {
        Self { peaks: [SILENCE_DB; 5], ..Default::default() }
    }

    /// One hop, `dt` seconds after the last.
    pub fn hop(&mut self, hop: &Hop, dt: f32) {
        let levels = [hop.level_db, hop.bands_db[0], hop.bands_db[1], hop.bands_db[2], hop.tonal_db];
        let mix = (self.peaks[0] - PEAK_RELEASE_DB * dt).max(hop.level_db);
        for (i, ((peak, smooth), level)) in self.peaks.iter_mut().zip(&mut self.smooth).zip(levels).enumerate() {
            let floor = if i == 0 { SILENCE_DB } else { (mix - BAND_RANGE_DB).max(SILENCE_DB) };
            *peak = (*peak - PEAK_RELEASE_DB * dt).max(level).max(floor);
            // amplitude relative to the recent peak; nothing at all when the peak itself is silence.
            // The model's melody is already 0..1, how much of the sound is tune
            let target = match hop.melody {
                Some(m) if i == 4 => {
                    if hop.level_db > SILENCE_DB { m } else { 0.0 }
                }
                _ if *peak <= SILENCE_DB + 1.0 => 0.0,
                _ => 10f32.powf((level - *peak) / 20.0),
            };
            let (attack, release) = if i == 4 { (MELODY_ATTACK_S, MELODY_RELEASE_S) } else { (ATTACK_S, RELEASE_S) };
            let tau = if target > *smooth { attack } else { release };
            *smooth += (target - *smooth) * (1.0 - (-dt / tau).exp());
        }
        let f = &mut self.features;
        [f.level, f.bass, f.mid, f.high, f.melody] = self.smooth;
        let fall = (-dt / HIT_DECAY_S).exp();
        for (env, onset) in [&mut f.kick, &mut f.snare, &mut f.hat].into_iter().zip(hop.onsets) {
            *env = if onset.is_some() { 1.0 } else { *env * fall };
        }
        // clocks: a quarter speed in silence, up to 1.75× when the band is full
        for (clock, level) in [(&mut f.level_time, f.level), (&mut f.bass_time, f.bass), (&mut f.mid_time, f.mid), (&mut f.high_time, f.high)] {
            *clock += dt * (0.25 + 1.5 * level);
        }
    }

    pub fn features(&self) -> Features {
        self.features
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Analyzer, HOP, synth};

    #[test]
    fn the_loop_drives_levels_hits_and_clocks() {
        let rate = 48_000.0;
        let take = synth::techno(rate, 128.0, 10.0, 5);
        let (mut analyzer, mut tracker) = (Analyzer::new(rate), Tracker::new());
        let dt = HOP as f32 / rate;
        let (mut at_kicks, mut between, mut last) = (Vec::new(), Vec::new(), Features::default());
        analyzer.push(&take.samples, |hop| {
            tracker.hop(hop, dt);
            let f = tracker.features();
            for v in [f.level, f.bass, f.mid, f.high, f.kick, f.snare, f.hat] {
                assert!((0.0..=1.0).contains(&v), "{f:?}");
            }
            assert!(f.bass_time >= last.bass_time && f.level_time > last.level_time, "clocks only move forward");
            if hop.time > 2.0 {
                // right on a kick vs. halfway between two, in the sound the hits describe
                let phase = (hop.heard * 128.0 / 60.0).fract();
                if phase < 0.05 {
                    at_kicks.push(f.kick);
                } else if (0.4..0.6).contains(&phase) {
                    between.push(f.kick);
                }
            }
            last = f;
        });
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        assert!(mean(&at_kicks) > 0.6 && mean(&between) < 0.3, "kick envelope: {:.2} on kicks, {:.2} between", mean(&at_kicks), mean(&between));
        assert!(last.level > 0.1, "a loud loop reads as loud after auto-gain: {:.2}", last.level);
    }

    #[test]
    fn a_band_that_goes_quiet_reads_quiet() {
        // a breakdown: the bass 40 dB under the mix for half a minute, the mids going on
        let mut tracker = Tracker::new();
        let hop = |bass: f32| Hop { time: 0.0, heard: 0.0, level_db: -10.0, bands_db: [bass, -14.0, -30.0], onsets: [None; 3], strength: 0.0, signals: [0.0; 3], tonal_db: -120.0, melody: None };
        for _ in 0..2000 {
            tracker.hop(&hop(-12.0), 0.005);
        }
        for _ in 0..6000 {
            tracker.hop(&hop(-50.0), 0.005);
        }
        let f = tracker.features();
        assert!(f.bass < 0.2 && f.mid > 0.9, "bass {:.2}, mid {:.2}", f.bass, f.mid);
    }

    #[test]
    fn the_melody_follows_the_tune_not_the_drums() {
        let mut tracker = Tracker::new();
        let hop = |tonal_db: f32| Hop { time: 0.0, heard: 0.0, level_db: -10.0, bands_db: [-12.0, -14.0, -30.0], onsets: [None; 3], strength: 0.0, signals: [0.0; 3], tonal_db, melody: None };
        for _ in 0..400 {
            tracker.hop(&hop(-16.0), 0.005);
        }
        assert!(tracker.features().melody > 0.8, "a tune: {:.2}", tracker.features().melody);
        // the tune stops, the beat goes on: the melody falls away
        for _ in 0..800 {
            tracker.hop(&hop(-70.0), 0.005);
        }
        assert!(tracker.features().melody < 0.1, "drums alone: {:.2}", tracker.features().melody);
    }

    #[test]
    fn silence_reads_as_nothing() {
        let mut tracker = Tracker::new();
        let quiet = Hop { time: 0.0, heard: 0.0, level_db: -90.0, bands_db: [-90.0; 3], onsets: [None; 3], strength: 0.0, signals: [0.0; 3], tonal_db: -120.0, melody: None };
        for _ in 0..200 {
            tracker.hop(&quiet, 0.005);
        }
        let f = tracker.features();
        assert_eq!((f.level, f.bass, f.kick), (0.0, 0.0, 0.0));
    }
}
