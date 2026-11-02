"""The labelling teacher: a network that hears a whole stretch at once (both ways in time) and
writes down every kick, snare and hat in it, from the drums Demucs split off and the mix. It
learns on generated drums (drumgen.py) over real minutes' bass and melody, split by Demucs as a
real minute is, every hit known; then it labels the library's real minutes, where nobody wrote
the hits down, for the live model to learn from and be measured on.

    python teacher.py make <library> <out> <count>     # generated clips, split, with their hits
    python teacher.py train <clips> <epochs> <weights>  # held-out clips scored as it goes
    python teacher.py label <weights> <minutes>...      # labels.json in each real minute
"""
import glob, json, os, sys, time

import numpy as np
import soundfile as sf
import torch
from scipy.ndimage import maximum_filter1d
from torch import nn

import drumgen
import library
import stemlabels

SR, HOP, NFFT, BANDS = 44100, 256, 1024, 80
CLIP_S = 30.0
HITS = ("kick", "snare", "hat")
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"


def mel_filters():
    mel = lambda f: 2595.0 * np.log10(1.0 + f / 700.0)
    edges = 700.0 * (10 ** (np.linspace(mel(30.0), mel(16000.0), BANDS + 2) / 2595.0) - 1.0)
    freqs = np.arange(NFFT // 2 + 1) * SR / NFFT
    fb = np.zeros((BANDS, NFFT // 2 + 1), dtype=np.float32)
    for b in range(BANDS):
        lo, mid, hi = edges[b], edges[b + 1], edges[b + 2]
        fb[b] = np.clip(np.minimum((freqs - lo) / (mid - lo), (hi - freqs) / (hi - mid)), 0, None)
    return torch.from_numpy(fb).to(DEVICE)


FB = mel_filters()


def mel(x):
    """Log-mel (dB) of mono samples at SR, a frame every HOP centred on its time, as float16:
    each clip against its own loud parts, so the level it was mastered at doesn't matter."""
    x = torch.from_numpy(np.ascontiguousarray(x, dtype=np.float32)).to(DEVICE)
    spec = torch.stft(x, NFFT, HOP, window=torch.hann_window(NFFT, device=DEVICE), return_complex=True)
    db = 10 * torch.log10(FB @ (spec.real ** 2 + spec.imag ** 2) + 1e-10).T
    db = (db - torch.quantile(db.float().flatten()[::7], 0.98)) / 20.0
    return db.clamp(min=-5.0).half().cpu().numpy()


def make(root, out, count):
    os.makedirs(out, exist_ok=True)
    model, device = library.demucs()
    n = int(CLIP_S * SR)
    started, minutes = time.time(), []
    for i in range(count):
        dest = os.path.join(out, f"clip{i:05d}.npz")
        if os.path.exists(dest):
            continue
        # the minutes split so far (library.py may still be at it)
        if i % 100 == 0 or not minutes:
            minutes = [os.path.join(root, n) for n in sorted(os.listdir(root)) if os.path.exists(os.path.join(root, n, "meta.json"))]
        r = np.random.default_rng(i)
        folder = minutes[r.integers(len(minutes))]
        rest = sum(sf.read(os.path.join(folder, s + ".flac"), dtype="float32")[0] for s in ("bass", "melody"))
        at = int(r.integers(0, max(1, len(rest) - n)))
        rest = rest[at:at + n]
        if len(rest) < n // 2:
            continue
        x, _, hits = drumgen.mix(r, rest, SR)
        stem = library.separate(model, device, np.stack([x, x]))["drums"]
        sf.write(os.path.join(out, f"clip{i:05d}.flac"), x, SR, subtype="PCM_24")
        np.savez(dest, stem=mel(stem), mix=mel(x), kick=np.array(hits[0]), snare=np.array(hits[1]), hat=np.array(hits[2]))
        if i % 50 == 0:
            print(f"{i}/{count} clips, {time.time() - started:.0f} s", flush=True)


class Teacher(nn.Module):
    def __init__(self):
        super().__init__()
        conv = lambda i, o: nn.Sequential(nn.Conv2d(i, o, 3, padding=1), nn.BatchNorm2d(o), nn.ReLU())
        self.cnn = nn.Sequential(conv(2, 32), conv(32, 32), nn.MaxPool2d((1, 2)), conv(32, 64), conv(64, 64), nn.MaxPool2d((1, 2)), conv(64, 64), nn.MaxPool2d((1, 2)))
        self.proj = nn.Linear(64 * (BANDS // 8), 256)
        self.gru = nn.GRU(256, 128, num_layers=2, batch_first=True, bidirectional=True, dropout=0.1)
        self.out = nn.Linear(256, 3)

    def forward(self, x):
        h = self.cnn(x).permute(0, 2, 1, 3).flatten(2)
        h, _ = self.gru(torch.relu(self.proj(h)))
        return self.out(h)


def targets(clip, frames):
    """1 on each hit's frame, 0.5 beside it."""
    y = np.zeros((frames, 3), dtype=np.float32)
    for c, h in enumerate(HITS):
        for t in clip[h]:
            f = int(round(t * SR / HOP))
            if f < frames:
                y[f, c] = 1.0
                for g in (f - 1, f + 1):
                    if 0 <= g < frames:
                        y[g, c] = max(y[g, c], 0.5)
    return y


def predict(model, stem, mix):
    """Probabilities per frame for a whole minute, in overlapping stretches."""
    x = torch.from_numpy(np.stack([stem, mix]).astype(np.float32))[None].to(DEVICE)
    out, n, win, edge = np.zeros((stem.shape[0], 3), dtype=np.float32), stem.shape[0], 2048, 128
    with torch.no_grad():
        for s in range(0, n, win - 2 * edge):
            lo, hi = max(0, s - edge), min(n, s + win - edge)
            p = torch.sigmoid(model(x[:, :, lo:hi])).cpu().numpy()[0]
            keep = slice(s - lo, min(hi, s + win - 2 * edge) - lo)
            out[s:s + p[keep].shape[0]] = p[keep]
    return out


def peaks(p, threshold, gap_s=0.03):
    """Hit times (s): frames over the threshold that are their neighbourhood's highest."""
    found = []
    gap = int(gap_s * SR / HOP)
    for c in range(3):
        q = p[:, c]
        cand = np.flatnonzero((q >= threshold[c]) & (q == maximum_filter1d(q, 5, mode="nearest")))
        keep = []
        for t in sorted(cand, key=lambda t: -q[t]):
            if all(abs(t - k) > gap for k in keep):
                keep.append(t)
        found.append(sorted(t * HOP / SR for t in keep))
    return found


def matched(truth, found, tol=0.015):
    found, used, m = np.array(found), set(), 0
    for t in truth:
        ok = [j for j in np.flatnonzero(np.abs(found - t) <= tol) if j not in used] if len(found) else []
        if ok:
            used.add(ok[0])
            m += 1
    return np.array([m, len(found), len(truth)])


def scores(model, clips, threshold, preds=None):
    """F, precision and recall per drum; `preds`, the clips' probabilities if already made."""
    tot = np.zeros((3, 3))
    preds = preds or [predict(model, c["stem"], c["mix"]) for c in clips]
    for c, p in zip(clips, preds):
        found = peaks(p, threshold)
        tot += np.stack([matched(c[h], found[k]) for k, h in enumerate(HITS)])
    out = []
    for m, f, t in tot:
        p, r = m / max(f, 1), m / max(t, 1)
        out.append((2 * p * r / max(p + r, 1e-9), p, r))
    return out


def train(data, epochs, weights):
    paths = sorted(glob.glob(os.path.join(data, "clip*.npz")))
    clips = [dict(np.load(p)) for p in paths]
    held = clips[: max(20, len(clips) // 12)]
    clips = clips[len(held):]
    print(len(clips), "clips to learn from,", len(held), "held out", flush=True)
    model = Teacher().to(DEVICE)
    opt = torch.optim.AdamW(model.parameters(), lr=1e-3, weight_decay=1e-4)
    steps = epochs * len(clips) * 4 // 16
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=1e-3, total_steps=max(1, steps))
    rng = np.random.default_rng(0)
    loss_fn = nn.BCEWithLogitsLoss()
    win, best = 512, -1.0
    for epoch in range(epochs):
        model.train()
        started, total, count = time.time(), 0.0, 0
        order = np.repeat(np.arange(len(clips)), 4)
        rng.shuffle(order)
        for s in range(0, len(order) - 15, 16):
            xs, ys = [], []
            for k in order[s:s + 16]:
                c = clips[k]
                n = c["stem"].shape[0]
                at = int(rng.integers(0, max(1, n - win)))
                x = np.stack([c["stem"][at:at + win], c["mix"][at:at + win]]).astype(np.float32)
                y = targets(c, n)[at:at + win]
                # a few dB either way, now and then the mix alone or the stem alone muffled
                x = x + rng.normal(0, 0.1)
                if rng.random() < 0.1:
                    x[0] = x[0] - rng.uniform(0.3, 1.0)
                xs.append(x)
                ys.append(y)
            xb = torch.from_numpy(np.stack(xs)).to(DEVICE)
            yb = torch.from_numpy(np.stack(ys)).to(DEVICE)
            loss = loss_fn(model(xb), yb)
            opt.zero_grad()
            loss.backward()
            opt.step()
            sched.step()
            total += loss.item()
            count += 1
        model.eval()
        v = scores(model, held, (0.5, 0.5, 0.5))
        mean = sum(f for f, _, _ in v) / 3
        if mean > best:
            best = mean
            torch.save(model.state_dict(), weights)
        print(f"epoch {epoch + 1}: loss {total / max(1, count):.4f}, {time.time() - started:.0f} s; held out " + "  ".join(f"{h} F {f:.3f} (p {p:.2f} r {r:.2f})" for h, (f, p, r) in zip(HITS, v)), flush=True)
    model.load_state_dict(torch.load(weights))
    model.eval()
    preds = [predict(model, c["stem"], c["mix"]) for c in held]
    tops = []
    for c in range(3):
        best_th = max(((scores(model, held, [th if k == c else 0.5 for k in range(3)], preds)[c][0], th) for th in np.arange(0.2, 0.85, 0.05)))
        tops.append(round(float(best_th[1]), 2))
    json.dump({"thresholds": tops}, open(weights + ".json", "w"))
    print("thresholds", tops, "held out at them:", "  ".join(f"{h} F {f:.3f}" for h, (f, _, _) in zip(HITS, scores(model, held, tops, preds))), flush=True)


def label(weights, roots):
    model = Teacher().to(DEVICE)
    model.load_state_dict(torch.load(weights))
    model.eval()
    threshold = json.load(open(weights + ".json"))["thresholds"]
    for root in roots:
        for name in sorted(os.listdir(root)):
            folder = os.path.join(root, name)
            if not os.path.exists(os.path.join(folder, "meta.json")):
                continue
            stem = mel(sf.read(os.path.join(folder, "drums.flac"), dtype="float32")[0])
            mix = mel(sf.read(os.path.join(folder, "mix.flac"), dtype="float32")[0])
            kick, snare, hat = peaks(predict(model, stem, mix), threshold)
            notes, level = stemlabels.melody(sf.read(os.path.join(folder, "melody.flac"), dtype="float32")[0], SR)
            out = {"kick": [round(t, 4) for t in kick], "snare": [round(t, 4) for t in snare], "hat": [round(t, 4) for t in hat], "melody": [round(t, 4) for t in notes], "melody_db": level}
            json.dump(out, open(os.path.join(folder, "labels.json"), "w"))
            print(name, {h: len(out[h]) for h in ("kick", "snare", "hat", "melody")}, flush=True)


if __name__ == "__main__":
    what = sys.argv[1]
    if what == "make":
        make(sys.argv[2], sys.argv[3], int(sys.argv[4]))
    elif what == "train":
        train(sys.argv[2], int(sys.argv[3]), sys.argv[4])
    elif what == "label":
        label(sys.argv[2], sys.argv[3:])
