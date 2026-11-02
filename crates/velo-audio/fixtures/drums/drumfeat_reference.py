"""The drum model's features, exactly as Velo computes them live (velo-audio must match this):
every hop of 256 samples, a 1024-point Hann spectrum of the last 1024 samples, 64 mel bands
from 30 Hz to 16 kHz in dB; each band against its running mean (2 s), its rise since the last
hop, and a slower view of the first (100 ms)."""
import numpy as np

HOP, WIN, BANDS = 256, 1024, 64
FMIN, FMAX = 30.0, 16000.0
CONTEXT = 6


def mel(f):
    return 2595.0 * np.log10(1.0 + f / 700.0)


def filterbank(sr):
    fmax = min(FMAX, sr / 2.0)
    edges = 700.0 * (10 ** (np.linspace(mel(FMIN), mel(fmax), BANDS + 2) / 2595.0) - 1.0)
    freqs = np.arange(WIN // 2 + 1) * sr / WIN
    fb = np.zeros((BANDS, WIN // 2 + 1), dtype=np.float32)
    for b in range(BANDS):
        lo, mid, hi = edges[b], edges[b + 1], edges[b + 2]
        up = (freqs - lo) / max(mid - lo, 1e-9)
        down = (hi - freqs) / max(hi - mid, 1e-9)
        fb[b] = np.clip(np.minimum(up, down), 0.0, None)
    return fb


def frames_db(x, sr):
    """Each hop's band levels (dB), the window ending at the hop's end (zeros before the start)."""
    n = len(x) // HOP
    padded = np.concatenate([np.zeros(WIN - HOP, dtype=np.float32), x.astype(np.float32)])
    idx = np.arange(WIN)[None, :] + HOP * np.arange(n)[:, None]
    window = (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(WIN) / WIN)).astype(np.float32)
    out = np.empty((n, BANDS), dtype=np.float32)
    fb = filterbank(sr)
    for s in range(0, n, 4096):
        spec = np.fft.rfft(padded[idx[s:s + 4096]] * window, axis=1)
        power = (spec.real ** 2 + spec.imag ** 2) / WIN
        out[s:s + 4096] = 10.0 * np.log10(power @ fb.T + 1e-10)
    return out


def ema(x, tau_frames, start):
    a = 1.0 - np.exp(-1.0 / tau_frames)
    out = np.empty_like(x)
    m = start.copy()
    for t in range(len(x)):
        out[t] = m  # the mean before this frame
        m = m + a * (x[t] - m)
    return out


def base(x, sr):
    """Per hop: relative level (64), rise (64), slow relative level (64), scaled /20."""
    db = frames_db(x, sr)
    hops_per_s = sr / HOP
    mean = ema(db, 2.0 * hops_per_s, db[0])
    rel = (db - mean) / 20.0
    rise = np.maximum(0.0, np.diff(db, axis=0, prepend=db[:1])) / 20.0
    slow = ema(rel, 0.1 * hops_per_s, np.zeros(BANDS, dtype=np.float32))
    return np.concatenate([rel, rise, slow], axis=1).astype(np.float32)


def stack(b, t):
    """The model's input at hop t: the last CONTEXT hops' level and rise, and the slow level now."""
    lo = t - CONTEXT + 1
    if lo < 0:
        pad = np.zeros((-lo, b.shape[1]), dtype=np.float32)
        win = np.concatenate([pad, b[: t + 1]])
    else:
        win = b[lo: t + 1]
    return np.concatenate([win[:, :128].reshape(-1), b[t, 128:]])


INPUTS = CONTEXT * 128 + 64
