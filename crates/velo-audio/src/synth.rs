//! A synthetic four-on-the-floor loop with known hit times. It's the onset tests' ground truth
//! (no annotated recordings to license) and a signal for the listen example without a device.

use std::f32::consts::TAU;

pub struct Take {
    /// Mono, peak 0.9.
    pub samples: Vec<f32>,
    /// When each kick, snare and hat starts, in seconds.
    pub kicks: Vec<f64>,
    pub snares: Vec<f64>,
    pub hats: Vec<f64>,
}

/// A state-variable filter (Chamberlin's): the low, band and high pass of what goes through it.
fn svf(freq: f32, damping: f32, rate: f32) -> impl FnMut(f32) -> [f32; 3] {
    let k = 2.0 * (std::f32::consts::PI * freq / rate).sin();
    let (mut low, mut band) = (0.0f32, 0.0f32);
    move |x| {
        low += k * band;
        let high = x - low - damping * band;
        band += k * high;
        [low, band, high]
    }
}

/// A techno loop: a kick on every beat, a snare on 2 and 4, open hats on the off-beats, a
/// rolling bass on the 16ths between kicks, a pad and a little noise underneath.
pub fn techno(rate: f32, bpm: f32, seconds: f32, seed: u32) -> Take {
    make(rate, bpm, seconds, seed, false)
}

/// The same loop with a clap on 2 and 4 instead of the snare (`snares` has its times).
pub fn claps(rate: f32, bpm: f32, seconds: f32, seed: u32) -> Take {
    make(rate, bpm, seconds, seed, true)
}

fn make(rate: f32, bpm: f32, seconds: f32, seed: u32, clap: bool) -> Take {
    let n = (seconds * rate) as usize;
    let mut out = vec![0.0f32; n];
    let mut rng = seed.max(1);
    let mut noise = move || {
        // xorshift32: enough for test noise
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let beat = 60.0 / bpm;
    let beats = (seconds / beat) as usize;
    let mut take = Take { samples: Vec::new(), kicks: Vec::new(), snares: Vec::new(), hats: Vec::new() };
    let mut add = |at: f32, len: f32, voice: &mut dyn FnMut(f32) -> f32| {
        let start = (at * rate) as usize;
        for i in 0..((len * rate) as usize).min(n.saturating_sub(start)) {
            out[start + i] += voice(i as f32 / rate);
        }
    };
    for b in 0..beats {
        let t = b as f32 * beat;
        // kick: a pitch drop from 160 to 50 Hz with a click
        take.kicks.push(f64::from(t));
        let mut phase = 0.0;
        add(t, 0.35, &mut |s| {
            phase += TAU * (50.0 + 110.0 * (-s / 0.03).exp()) / rate;
            0.8 * phase.sin() * (-s / 0.18).exp() + if s < 0.002 { 0.3 * noise() } else { 0.0 }
        });
        if b % 2 == 1 && clap {
            take.snares.push(f64::from(t));
            // three hands a few milliseconds apart over the room, as a TR-808 makes it: noise
            // around 1.4 kHz
            let mut filter = svf(1400.0, 0.8, rate);
            add(t, 0.3, &mut |s| {
                let [_, band, _] = filter(noise());
                let hands = [0.0, 0.009, 0.018].iter().filter(|&&at| s >= at).map(|&at| (-(s - at) / 0.004).exp()).fold(0.0, f32::max);
                let room = 0.35 * (-s / 0.12).exp();
                2.5 * band * hands.max(room)
            });
        } else if b % 2 == 1 {
            take.snares.push(f64::from(t));
            // the wires' noise, rolled off above a few kHz as a snare's is, over its body
            let mut filter = svf(3000.0, 1.0, rate);
            add(t, 0.3, &mut |s| {
                let [wires, ..] = filter(noise());
                1.2 * wires * (-s / 0.1).exp() + 0.25 * (TAU * 190.0 * s).sin() * (-s / 0.05).exp()
            });
        }
        let off = t + beat / 2.0;
        take.hats.push(f64::from(off));
        // noise above 7 kHz
        let mut filter = svf(7000.0, 1.0, rate);
        add(off, 0.12, &mut |s| {
            let [.., high] = filter(noise());
            0.2 * high * (-s / 0.03).exp()
        });
        for sixteenth in 1..4 {
            let at = t + beat * sixteenth as f32 / 4.0;
            add(at, beat / 4.0, &mut |s| {
                let env = (s / 0.004).min(1.0) * (-s / 0.08).exp();
                // ~9 dB under the kick in the sub band, as in a typical techno mix
                0.18 * env * ((TAU * 55.0 * s).sin() + 0.4 * (TAU * 110.0 * s).sin())
            });
        }
    }
    for (i, x) in out.iter_mut().enumerate() {
        let s = i as f32 / rate;
        let swell = 0.5 + 0.5 * (TAU * s / 8.0).sin();
        *x += 0.03 * swell * [220.0, 277.18, 329.63].iter().map(|f| (TAU * f * s).sin()).sum::<f32>() + 0.002 * noise();
    }
    let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-9);
    out.iter_mut().for_each(|x| *x *= 0.9 / peak);
    take.samples = out;
    take
}
