//! A short spectrum every hop (512 points, Hann, about 11 ms at 48 kHz): the peaks that stand
//! over their neighbours are the tonal part of the sound, the melody. A long one (4096 points,
//! about 12 Hz a bin) parts the low end, where the short one's 94-Hz bins lump a sub and a kick.

use std::f32::consts::TAU;

/// Points per short spectrum.
pub const N: usize = 512;
/// Bins per short spectrum: 0 Hz to half the rate.
pub const BINS: usize = N / 2 + 1;
/// Points per long spectrum: 11.7 Hz a bin at 48 kHz, over 85 ms. 8192 would halve the bins but
/// smear a kick over 170 ms; the engine only reads it under 500 Hz, where a period is long anyway.
pub const LONG: usize = 4096;

/// What one hop's spectrum says.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reading {
    /// The energy of the peaks between 200 Hz and 4 kHz (dB): the tonal part, the melody.
    pub tonal_db: f32,
}

/// Turns samples into a spectrum (and a [`Reading`]) per hop. Allocates nothing after `new`.
pub struct Spectrum {
    rate: f32,
    ring: Vec<f32>,
    at: usize,
    window: Vec<f32>,
    rev: Vec<usize>,
    cos: Vec<f32>,
    sin: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    power: Vec<f32>,
}

impl Spectrum {
    pub fn new(rate: f32) -> Self {
        Self::with_points(rate, N)
    }

    /// A spectrum over the last `n` samples, `n` a power of two.
    pub fn with_points(rate: f32, n: usize) -> Self {
        assert!(n.is_power_of_two(), "a radix-2 FFT: {n} points");
        let bits = n.trailing_zeros();
        Self {
            rate,
            ring: vec![0.0; n],
            at: 0,
            window: (0..n).map(|i| 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos()).collect(),
            rev: (0..n).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect(),
            cos: (0..n / 2).map(|j| (TAU * j as f32 / n as f32).cos()).collect(),
            sin: (0..n / 2).map(|j| (TAU * j as f32 / n as f32).sin()).collect(),
            re: vec![0.0; n],
            im: vec![0.0; n],
            power: vec![0.0; n / 2 + 1],
        }
    }

    pub fn push(&mut self, x: f32) {
        self.ring[self.at] = x;
        self.at = (self.at + 1) % self.ring.len();
    }

    /// The last samples' spectrum, and its tonal part.
    pub fn hop(&mut self) -> Reading {
        self.transform();
        let bins = self.power.len();
        let bin = |hz: f32| ((hz / self.rate * self.ring.len() as f32) as usize).min(bins - 1);
        // the peaks: a bin four times its neighbourhood's median, or more
        let mut tonal = 0.0;
        let mut near = [0.0f32; 12];
        for k in bin(200.0).max(6)..=bin(4000.0).min(bins - 7) {
            near[..6].copy_from_slice(&self.power[k - 6..k]);
            near[6..].copy_from_slice(&self.power[k + 1..k + 7]);
            let (_, median, _) = near.select_nth_unstable_by(6, f32::total_cmp);
            if self.power[k] > 4.0 * *median {
                tonal += self.power[k];
            }
        }
        Reading { tonal_db: 10.0 * tonal.max(1e-12).log10() }
    }

    /// The last samples' power per bin, without the tonal part.
    pub fn transform(&mut self) {
        let n = self.ring.len();
        for i in 0..n {
            self.re[self.rev[i]] = self.ring[(self.at + i) % n] * self.window[i];
            self.im[self.rev[i]] = 0.0;
        }
        self.fft();
        for (k, p) in self.power.iter_mut().enumerate() {
            *p = (self.re[k] * self.re[k] + self.im[k] * self.im[k]) / n as f32;
        }
    }

    /// The last transform's power per bin (bin k is k × rate / points Hz).
    pub fn power(&self) -> &[f32] {
        &self.power
    }

    /// In place, the input in bit-reversed order (radix 2).
    fn fft(&mut self) {
        let n = self.ring.len();
        let mut len = 2;
        while len <= n {
            let step = n / len;
            for start in (0..n).step_by(len) {
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

    #[test]
    fn the_long_spectrum_parts_a_sub_from_a_kick() {
        // 40 Hz and 80 Hz fall in the short spectrum's first bin; the long one sets them apart
        let mut s = Spectrum::with_points(48_000.0, LONG);
        for i in 0..LONG {
            s.push((TAU * 40.0 * i as f32 / 48_000.0).sin());
        }
        s.transform();
        let hz = |k: usize| k as f32 * 48_000.0 / LONG as f32;
        let loudest = (0..s.power().len()).max_by(|&a, &b| s.power()[a].total_cmp(&s.power()[b])).unwrap();
        assert!((hz(loudest) - 40.0).abs() < 6.0, "the peak at {} Hz", hz(loudest));
        let at_80 = s.power()[(80.0 * LONG as f32 / 48_000.0).round() as usize];
        assert!(at_80 < s.power()[loudest] * 1e-3, "80 Hz is 30 dB under the tone");
    }
}
