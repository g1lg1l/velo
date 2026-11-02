"""Scores Velo's own detectors (the listen example, so exactly the code the app runs) on library
minutes labelled by teacher.py: kick, snare and hat per mode, against the teacher's hits, and the
melody against the melody stem's level.

    python score.py <listen.exe> <folder of minutes> [simple|model] [-- listen flags...]

A hit counts when the detector fires from 10 ms before a labelled one to 40 ms after it: the
labels are offline and exact to a few milliseconds, the detectors live.
"""
import json, os, subprocess, sys

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

HITS = ("kick", "snare", "hat")
EARLY, LATE = 0.010, 0.040


def wav48(folder):
    """The minute's mix as 16-bit mono 48 kHz WAV, what the example reads (made once)."""
    path = os.path.join(folder, "mix48.wav")
    if not os.path.exists(path):
        x, sr = sf.read(os.path.join(folder, "mix.flac"), dtype="float32")
        sf.write(path, np.clip(resample_poly(x, 160, 147), -1, 1), 48000, subtype="PCM_16")
    return path


def found(listen, path, flags):
    out = subprocess.run([listen, "--file", path, *flags], capture_output=True, text=True).stdout
    hits = {h: [] for h in HITS}
    for line in out.splitlines()[1:]:
        t, hit, _ = line.split(",")
        hits[hit].append(float(t))
    return hits


def matched(truth, hits):
    """How many of `truth` a hit answers (each hit answers one), of how many hits and labels."""
    hits, used, m = np.array(sorted(hits)), set(), 0
    for t in sorted(truth):
        ok = np.flatnonzero((hits >= t - EARLY) & (hits <= t + LATE))
        ok = [j for j in ok if j not in used]
        if ok:
            used.add(ok[0])
            m += 1
    return np.array([m, len(hits), len(truth)])


def report(name, tot):
    parts = []
    for c, h in enumerate(HITS):
        m, f, t = tot[c]
        p, r = m / max(f, 1), m / max(t, 1)
        parts.append(f"{h} F {2 * p * r / max(p + r, 1e-9):.2f} (p {p:.2f} r {r:.2f})")
    print(f"{name:<48} " + "  ".join(parts), flush=True)


def run(listen, root, flags):
    tot, by = np.zeros((3, 3)), {}
    for name in sorted(os.listdir(root)):
        folder = os.path.join(root, name)
        if not os.path.exists(os.path.join(folder, "labels.json")):
            continue
        labels = json.load(open(os.path.join(folder, "labels.json")))
        genre = json.load(open(os.path.join(folder, "meta.json"))).get("folder", "")
        hits = found(listen, wav48(folder), flags)
        one = np.stack([matched(labels[h], hits[h]) for h in HITS])
        tot += one
        by[genre] = by.get(genre, 0) + one
    for genre in sorted(by):
        report(genre[:48], by[genre])
    report("ALL", tot)
    return tot


def rank(v):
    return np.argsort(np.argsort(v)).astype(np.float64)


def melody(listen, root, flags):
    """How well the melody feature follows the melody stem's level: Spearman correlation per
    minute (the stem's level every 10 ms against the feature then), averaged."""
    corr = []
    for name in sorted(os.listdir(root)):
        folder = os.path.join(root, name)
        if not os.path.exists(os.path.join(folder, "labels.json")):
            continue
        truth = np.array(json.load(open(os.path.join(folder, "labels.json")))["melody_db"])
        out = subprocess.run([listen, "--file", wav48(folder), "--features", *flags], capture_output=True, text=True).stdout
        rows = np.array([[float(v) for v in line.split(",")] for line in out.splitlines()[1:]])
        t = np.arange(len(truth)) * 0.01
        feat = np.interp(t, rows[:, 0], rows[:, 8])
        keep = t > 2.0
        if keep.sum() > 100 and np.std(truth[keep]) > 0.5:
            corr.append(np.corrcoef(rank(truth[keep]), rank(feat[keep]))[0, 1])
    print(f"melody follows the melody stem: Spearman {np.mean(corr):.2f} over {len(corr)} minutes", flush=True)


if __name__ == "__main__":
    listen, root = sys.argv[1], sys.argv[2]
    mode = sys.argv[3] if len(sys.argv) > 3 else "simple"
    extra = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    flags = (["--model"] if mode == "model" else []) + extra
    run(listen, root, flags)
    melody(listen, root, flags)
