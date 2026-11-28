//! Live drum probabilities from a Hann spectrum, mel features and a small recurrent network (a
//! layer over the last six hops, then a GRU, its memory of the bar so far): kick, snare and hat,
//! and how much of the sound is melody.

use std::f64::consts::TAU;

const HOP: usize = 256;
const WIN: usize = 1024;
const BINS: usize = WIN / 2 + 1;
const BANDS: usize = 64;
const BASE: usize = 3 * BANDS;
const CONTEXT: usize = 6;
const INPUTS: usize = CONTEXT * BASE;
const FRONT: usize = 256;
/// The memory: its size, and how many GRUs deep.
const STATE: usize = 256;
const LAYERS: usize = 2;
const GATES: usize = 3 * STATE;
/// Kick, snare, hat, then the melody.
pub const OUTPUTS: usize = 4;
const WEIGHTS: usize = FRONT * (INPUTS + 1) + gru_weights(FRONT) + (LAYERS - 1) * gru_weights(STATE) + OUTPUTS * (FRONT + STATE + 1);

const fn gru_weights(inputs: usize) -> usize {
    GATES * (inputs + STATE + 2)
}

struct Band {
    start: usize,
    weights: Box<[f32]>,
}

/// Turns mono samples into kick, snare and hat probabilities and the melody (0..1). Allocates
/// nothing after `new`.
pub struct DrumNet {
    weights: Box<[f32]>,
    bands: [Band; BANDS],
    window: [f32; WIN],
    rev: [usize; WIN],
    cos: [f32; WIN / 2],
    sin: [f32; WIN / 2],
    ring: [f32; WIN],
    at: usize,
    pending: usize,
    re: [f32; WIN],
    im: [f32; WIN],
    power: [f32; BINS],
    first: bool,
    mean: [f32; BANDS],
    previous: [f32; BANDS],
    slow: [f32; BANDS],
    mean_a: f32,
    slow_a: f32,
    base: [f32; BASE],
    input: [f32; INPUTS],
    /// The front layer's answer, then the last GRU's state: what the output layer reads.
    joined: [f32; FRONT + STATE],
    state: [[f32; STATE]; LAYERS],
}

impl DrumNet {
    /// Loads little-endian f32 weights in PyTorch's order (rows, then biases): the front layer
    /// (each row hop by hop, the oldest first), each GRU (input rows, state rows, their biases),
    /// the output layer.
    pub fn new(rate: f32, weights: &[u8]) -> Result<Self, String> {
        if weights.len() != WEIGHTS * 4 {
            return Err(format!("expected {} weight bytes, got {}", WEIGHTS * 4, weights.len()));
        }
        if !rate.is_finite() || rate <= 0.0 {
            return Err("sample rate must be finite and positive".into());
        }
        let rate = rate as f64;
        let mel = |f: f64| 2595.0 * (1.0 + f / 700.0).log10();
        let lo = mel(30.0);
        let step = (mel(16_000.0_f64.min(rate / 2.0)) - lo) / (BANDS + 1) as f64;
        let edges: [f64; BANDS + 2] = std::array::from_fn(|i| 700.0 * (10.0_f64.powf((lo + i as f64 * step) / 2595.0) - 1.0));
        let bands = std::array::from_fn(|b| {
            let mut start = 0;
            let mut weights = Vec::new();
            for k in 0..BINS {
                let f = k as f64 * rate / WIN as f64;
                let up = (f - edges[b]) / (edges[b + 1] - edges[b]).max(1e-9);
                let down = (edges[b + 2] - f) / (edges[b + 2] - edges[b + 1]).max(1e-9);
                let weight = up.min(down).max(0.0) as f32;
                if weight > 0.0 {
                    if weights.is_empty() {
                        start = k;
                    }
                    weights.push(weight);
                }
            }
            Band { start, weights: weights.into_boxed_slice() }
        });
        let hops_per_s = rate / HOP as f64;
        Ok(Self {
            weights: weights.as_chunks::<4>().0.iter().map(|&v| f32::from_le_bytes(v)).collect(),
            bands,
            window: std::array::from_fn(|i| (0.5 - 0.5 * (TAU * i as f64 / WIN as f64).cos()) as f32),
            rev: std::array::from_fn(|i| i.reverse_bits() >> (usize::BITS - WIN.trailing_zeros())),
            cos: std::array::from_fn(|i| (TAU * i as f64 / WIN as f64).cos() as f32),
            sin: std::array::from_fn(|i| (TAU * i as f64 / WIN as f64).sin() as f32),
            ring: [0.0; WIN],
            at: 0,
            pending: 0,
            re: [0.0; WIN],
            im: [0.0; WIN],
            power: [0.0; BINS],
            first: true,
            mean: [0.0; BANDS],
            previous: [0.0; BANDS],
            slow: [0.0; BANDS],
            mean_a: (1.0 - (-1.0 / (2.0 * hops_per_s)).exp()) as f32,
            slow_a: (1.0 - (-1.0 / (0.1 * hops_per_s)).exp()) as f32,
            base: [0.0; BASE],
            input: [0.0; INPUTS],
            joined: [0.0; FRONT + STATE],
            state: [[0.0; STATE]; LAYERS],
        })
    }

    /// Feeds one sample, returning probabilities whenever a hop of 256 samples completes.
    pub fn push(&mut self, x: f32) -> Option<[f32; OUTPUTS]> {
        self.ring[self.at] = x;
        self.at = (self.at + 1) % WIN;
        self.pending += 1;
        if self.pending < HOP {
            return None;
        }
        self.pending = 0;
        self.features();
        self.input.copy_within(BASE.., 0);
        self.input[INPUTS - BASE..].copy_from_slice(&self.base);
        let (w, rest) = self.weights.split_at(FRONT * INPUTS);
        let (b, mut rest) = rest.split_at(FRONT);
        let (front, top) = self.joined.split_at_mut(FRONT);
        linear(&self.input, w, b, front);
        for x in front.iter_mut() {
            *x = x.max(0.0);
        }
        for l in 0..LAYERS {
            let (below, here) = self.state.split_at_mut(l);
            let input: &[f32] = if l == 0 { front } else { &below[l - 1] };
            let (w, after) = rest.split_at(gru_weights(input.len()));
            gru(input, w, &mut here[0]);
            rest = after;
        }
        top.copy_from_slice(&self.state[LAYERS - 1]);
        let mut out = [0.0; OUTPUTS];
        let (w, b) = rest.split_at(OUTPUTS * (FRONT + STATE));
        linear(&self.joined, w, b, &mut out);
        for x in &mut out {
            *x = 1.0 / (1.0 + (-*x).exp());
        }
        Some(out)
    }

    /// The last hop's relative levels, rises and slow levels, or zeros before the first hop.
    pub fn last_base(&self) -> &[f32] {
        &self.base
    }

    fn features(&mut self) {
        for i in 0..WIN {
            self.re[self.rev[i]] = self.ring[(self.at + i) % WIN] * self.window[i];
            self.im[self.rev[i]] = 0.0;
        }
        self.fft();
        for (k, power) in self.power.iter_mut().enumerate() {
            *power = (self.re[k] * self.re[k] + self.im[k] * self.im[k]) / WIN as f32;
        }
        for (b, band) in self.bands.iter().enumerate() {
            let power: f32 = band.weights.iter().zip(&self.power[band.start..]).map(|(w, p)| w * p).sum();
            let db = 10.0 * (power + 1e-10).log10();
            if self.first {
                self.mean[b] = db;
                self.previous[b] = db;
            }
            let rel = (db - self.mean[b]) / 20.0;
            self.base[b] = rel;
            self.base[BANDS + b] = (db - self.previous[b]).max(0.0) / 20.0;
            self.base[2 * BANDS + b] = self.slow[b];
            self.mean[b] += self.mean_a * (db - self.mean[b]);
            self.slow[b] += self.slow_a * (rel - self.slow[b]);
            self.previous[b] = db;
        }
        self.first = false;
    }

    /// In place, with the input in bit-reversed order (radix 2).
    fn fft(&mut self) {
        let mut len = 2;
        while len <= WIN {
            let step = WIN / len;
            for start in (0..WIN).step_by(len) {
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

fn linear(input: &[f32], weights: &[f32], bias: &[f32], output: &mut [f32]) {
    for ((row, bias), out) in weights.chunks_exact(input.len()).zip(bias).zip(output) {
        // eight sums side by side: one sum in order keeps the compiler off the wide registers
        let mut sums = [0.0f32; 8];
        let ((rows, row_rest), (inputs, input_rest)) = (row.as_chunks::<8>(), input.as_chunks::<8>());
        let rest: f32 = row_rest.iter().zip(input_rest).map(|(w, x)| w * x).sum();
        for (w, x) in rows.iter().zip(inputs) {
            for ((sum, w), x) in sums.iter_mut().zip(w).zip(x) {
                *sum += w * x;
            }
        }
        *out = sums.iter().sum::<f32>() + rest + bias;
    }
}

/// One GRU step, PyTorch's: input rows, state rows, then their biases; the gates r, z, n.
fn gru(input: &[f32], weights: &[f32], state: &mut [f32; STATE]) {
    let (wi, rest) = weights.split_at(GATES * input.len());
    let (wh, bias) = rest.split_at(GATES * STATE);
    let (mut a, mut b) = ([0.0; GATES], [0.0; GATES]);
    linear(input, wi, &bias[..GATES], &mut a);
    linear(&state[..], wh, &bias[GATES..], &mut b);
    let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
    for (i, h) in state.iter_mut().enumerate() {
        let (r, z) = (sigmoid(a[i] + b[i]), sigmoid(a[STATE + i] + b[STATE + i]));
        *h = (1.0 - z) * (a[2 * STATE + i] + r * b[2 * STATE + i]).tanh() + z * *h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(bytes: &[u8]) -> Vec<f32> {
        assert_eq!(bytes.len() % 4, 0);
        bytes.as_chunks::<4>().0.iter().map(|&v| f32::from_le_bytes(v)).collect()
    }

    fn wav(bytes: &[u8]) -> Vec<f32> {
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let u16_at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap());
        let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
        let end = 8 + u32_at(4) as usize;
        assert_eq!(end, bytes.len());
        let mut at = 12;
        let mut format = false;
        let mut data = None;
        while at < end {
            let len = u32_at(at + 4) as usize;
            let start = at + 8;
            assert!(start + len <= end);
            match &bytes[at..at + 4] {
                b"fmt " => {
                    assert!(len >= 16);
                    assert_eq!(u16_at(start), 1);
                    assert_eq!(u16_at(start + 2), 1);
                    assert_eq!(u32_at(start + 4), 48_000);
                    assert_eq!(u16_at(start + 12), 2);
                    assert_eq!(u16_at(start + 14), 16);
                    format = true;
                }
                b"data" => data = Some(&bytes[start..start + len]),
                _ => {}
            }
            at = start + len + len % 2;
        }
        assert!(format);
        let data = data.unwrap();
        assert_eq!(data.len() % 2, 0);
        data.as_chunks::<2>().0.iter().map(|&v| i16::from_le_bytes(v) as f32 / 32768.0).collect()
    }

    #[test]
    fn matches_python_every_hop() {
        let samples = wav(include_bytes!("../fixtures/drums/drums.wav"));
        let base = floats(include_bytes!("../fixtures/drums/drums-base.bin"));
        let expected = floats(include_bytes!("../fixtures/drums/drums-expected.bin"));
        let mut net = DrumNet::new(48_000.0, include_bytes!("../fixtures/drums/drums-random.bin")).unwrap();
        assert_eq!(samples.len(), 750 * HOP);
        assert_eq!(base.len(), 750 * 192);
        assert_eq!(expected.len(), 750 * OUTPUTS);
        assert!(net.last_base().iter().all(|&v| v == 0.0));
        let mut hops = 0;
        for (i, x) in samples.into_iter().enumerate() {
            let output = net.push(x);
            assert_eq!(output.is_some(), (i + 1) % HOP == 0);
            if let Some(output) = output {
                for (b, (&actual, &want)) in net.last_base().iter().zip(&base[hops * 192..][..192]).enumerate() {
                    assert!((actual - want).abs() <= 1e-3, "hop {hops}, base {b}: {actual} != {want}");
                }
                for (b, (&actual, &want)) in output.iter().zip(&expected[hops * OUTPUTS..][..OUTPUTS]).enumerate() {
                    assert!((actual - want).abs() <= 1e-4, "hop {hops}, output {b}: {actual} != {want}");
                }
                if hops == 0 {
                    assert!(net.last_base().iter().all(|&v| v == 0.0));
                }
                hops += 1;
            }
        }
        assert_eq!(hops, 750);
    }

    #[test]
    fn wrong_weights_length_is_an_error() {
        for len in [0, WEIGHTS * 4 - 1, WEIGHTS * 4 + 1] {
            assert!(DrumNet::new(48_000.0, &vec![0; len]).is_err());
        }
    }
}
