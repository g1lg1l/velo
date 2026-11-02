//! A short spectrum every hop (512 points, Hann, about 11 ms at 48 kHz): the peaks that stand
//! over their neighbours are the tonal part of the sound, the melody.

use std::f32::consts::TAU;

/// Points per spectrum.
pub const N: usize = 512;
/// Bins per spectrum: 0 Hz to half the rate.
pub const BINS: usize = N / 2 + 1;

/// What one hop's spectrum says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// The energy of the peaks between 200 Hz and 4 kHz (dB): the tonal part, the melody.
    pub tonal_db: f32,
}

/// Turns samples into a [`Reading`] per hop. Allocates nothing after `new`.
pub struct Spectrum {
    rate: f32,
    ring: [f32; N],
    at: usize,
    window: [f32; N],
    rev: [usize; N],
    cos: [f32; N / 2],
    sin: [f32; N / 2],
    re: [f32; N],
    im: [f32; N],
    power: [f32; BINS],
}

impl Spectrum {
    pub fn new(rate: f32) -> Self {
        let bits = N.trailing_zeros();
        Self {
            rate,
            ring: [0.0; N],
            at: 0,
            window: std::array::from_fn(|i| 0.5 - 0.5 * (TAU * i as f32 / N as f32).cos()),
            rev: std::array::from_fn(|i| i.reverse_bits() >> (usize::BITS - bits)),
            cos: std::array::from_fn(|j| (TAU * j as f32 / N as f32).cos()),
            sin: std::array::from_fn(|j| (TAU * j as f32 / N as f32).sin()),
            re: [0.0; N],
            im: [0.0; N],
            power: [0.0; BINS],
        }
    }

    pub fn push(&mut self, x: f32) {
        self.ring[self.at] = x;
        self.at = (self.at + 1) % N;
    }

    /// The last `N` samples' spectrum, against the one a hop before.
    pub fn hop(&mut self) -> Reading {
        for i in 0..N {
            self.re[self.rev[i]] = self.ring[(self.at + i) % N] * self.window[i];
            self.im[self.rev[i]] = 0.0;
        }
        self.fft();
        for (k, p) in self.power.iter_mut().enumerate() {
            *p = (self.re[k] * self.re[k] + self.im[k] * self.im[k]) / N as f32;
        }
        let bin = |hz: f32| ((hz / self.rate * N as f32) as usize).min(BINS - 1);
        // the peaks: a bin four times its neighbourhood's median, or more
        let mut tonal = 0.0;
        let mut near = [0.0f32; 12];
        for k in bin(200.0).max(6)..=bin(4000.0).min(BINS - 7) {
            near[..6].copy_from_slice(&self.power[k - 6..k]);
            near[6..].copy_from_slice(&self.power[k + 1..k + 7]);
            let (_, median, _) = near.select_nth_unstable_by(6, f32::total_cmp);
            if self.power[k] > 4.0 * *median {
                tonal += self.power[k];
            }
        }
        Reading { tonal_db: 10.0 * tonal.max(1e-12).log10() }
    }

    /// The last hop's power per bin (bin k is k × rate / N Hz).
    pub fn power(&self) -> &[f32; BINS] {
        &self.power
    }

    /// In place, the input in bit-reversed order (radix 2).
    fn fft(&mut self) {
        let mut len = 2;
        while len <= N {
            let step = N / len;
            for start in (0..N).step_by(len) {
                for k in 0..len / 2 {
                    let (c, s) = (self.cos[k * step], -self.sin[k * step]);
                    let (a, b) = (start + k, start + k + len / 2);
                    let (tr, ti) = (self.re[b] * c - self.im[b] * s, self.re[b] * s + self.im[b] * c);
                    (self.re[b], self.im[b]) = (self.re[a] - tr, self.im[a] - ti);
                    self.re[a] += tr;
                    self.im[a] += ti;
                }
            }
            len <<= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(samples: impl Iterator<Item = f32>) -> Reading {
        let mut s = Spectrum::new(48_000.0);
        let mut last = Reading::default();
        for (i, x) in samples.enumerate() {
            s.push(x);
            if (i + 1) % 256 == 0 {
                last = s.hop();
            }
        }
        last
    }

    #[test]
    fn a_tone_is_a_peak() {
        // a 1 kHz tone: all its energy in a peak
        let tone = read((0..4096).map(|i| (TAU * 1000.0 * i as f32 / 48_000.0).sin()));
        assert!(tone.tonal_db > -10.0, "{tone:?}");
    }
}
