"""The melody's note starts and level, from a library minute's melody stem (library.py): what
score.py measures the live melody against, written into each minute's labels.json by teacher.py.
"""
import numpy as np

N, H = 1024, 128


def spectrum(x, sr):
    """Power per bin, a frame every H samples; frame t covers samples t*H .. t*H+N."""
    x = np.concatenate([np.zeros(N // 2, dtype=np.float32), x.astype(np.float32), np.zeros(N, dtype=np.float32)])
    n = (len(x) - N) // H
    idx = np.arange(N)[None, :] + H * np.arange(n)[:, None]
    win = np.hanning(N).astype(np.float32)
    out = np.empty((n, N // 2 + 1), dtype=np.float32)
    for s in range(0, n, 8192):
        spec = np.fft.rfft(x[idx[s:s + 8192]] * win, axis=1)
        out[s:s + 8192] = spec.real ** 2 + spec.imag ** 2
    # frame t is centred on sample t*H (the padding), so its time is t*H/sr
    return out, np.fft.rfftfreq(N, 1 / sr)


def peaks(r, threshold, gap):
    """Frames where `r` peaks over `threshold`, the larger of two closer than `gap` frames kept."""
    cand = np.flatnonzero((r >= threshold) & (r >= np.roll(r, 1)) & (r > np.roll(r, -1)))
    keep = []
    for t in cand[np.argsort(-r[cand])]:
        if all(abs(t - k) >= gap for k in keep):
            keep.append(t)
    return np.array(sorted(keep), dtype=int)


def melody(x, sr):
    """Note starts (s) and the level (dB, a value every 10 ms) of the melody stem."""
    p, f = spectrum(x, sr)
    fps = sr / H
    sel = (f >= 150) & (f < 8000)
    logp = np.log10(p[:, sel] + 1e-9)
    flux = np.maximum(np.diff(logp, axis=0, prepend=logp[:1]), 0).sum(axis=1)
    # against its own neighbourhood: a note start stands out of the last half second's flux
    k = int(0.5 * fps)
    med = np.array([np.median(flux[max(0, t - k):t + 1]) for t in range(len(flux))])
    level = 10 * np.log10(p[:, sel].sum(axis=1) + 1e-10)
    on = [t for t in peaks(flux - med, 0.0, int(0.06 * fps)) if flux[t] > 2.5 * med[t] + 1.0 and level[t] > np.percentile(level, 95) - 30]
    step = max(1, int(round(0.01 * fps)))
    return [t / fps for t in on], level[::step].round(1).tolist()
