"""Trains Velo's live model: drumtrain.py's small MLP over drumfeat's inputs, now with a fourth
output, how much of the sound is melody (livedata.py's target, learned where a melody stem says
it). Learns from generated clips (every hit exact) and real library minutes (hits from the
teacher); a tenth of the real minutes tune the thresholds; the test minutes only score it.

    python livetrain.py <train folders, comma separated> <epochs> <out> <test folder>...

Writes <out>.pt, <out>.bin (little-endian f32 in PyTorch Linear order, as drums.rs reads it) and
<out>.json (each drum's threshold).
"""
import glob, json, os, sys, time

import numpy as np
import torch
from torch import nn

import drumfeat as F
import drumtrain as T

HIDDEN = tuple(int(h) for h in os.environ.get("HIDDEN", "256,128").split(","))
OUTPUTS = 4
DEVICE = "cuda" if torch.cuda.is_available() else "cpu"


class Model(nn.Module):
    def __init__(self):
        super().__init__()
        a, b = HIDDEN
        self.net = nn.Sequential(nn.Linear(F.INPUTS, a), nn.ReLU(), nn.Dropout(0.1), nn.Linear(a, b), nn.ReLU(), nn.Linear(b, OUTPUTS))

    def forward(self, x):
        return self.net(x)


def one(p):
    """A track's features, targets (hits snapped to their rise, then the melody target) and
    which targets it has."""
    d = np.load(p, allow_pickle=True)
    x, sr = d["x"], int(d["sr"])
    if "folder" in d.files:
        # a real minute: the teacher's hits, as last labelled
        labels = json.load(open(os.path.join(str(d["folder"]), "labels.json")))
        hits = np.array([np.array(labels[h], dtype=np.float64) for h in ("kick", "snare", "hat")], dtype=object)
    else:
        hits = d["hits"]
    y = np.zeros((len(x), OUTPUTS), dtype=np.float32)
    y[:, :3] = T.snapped(x, hits, sr)
    mask = np.ones(OUTPUTS, dtype=np.float32)
    if "melody" in d.files:
        m = d["melody"].astype(np.float32)[: len(x)]
        # the model answers for the hop LOOKAHEAD before the newest
        y[T.LOOKAHEAD:len(m), 3] = m[: len(m) - T.LOOKAHEAD]
    else:
        mask[3] = 0.0
    return x, y, sr, hits, mask


def load(paths):
    return [(p, *one(p)) for p in paths]


def stacked(paths, crop, rng):
    """Every track (a random stretch of `crop` hops of each) end to end in one array, each after
    CONTEXT - 1 silent hops so any hop's window is one gather, filled as they load (memory is
    16 GB here); its targets alongside, where each track starts, and each track's masks."""
    pad = F.CONTEXT - 1
    big = np.zeros((len(paths) * (crop + pad), 192), dtype=np.float16)
    ys = np.zeros((len(paths) * (crop + pad), OUTPUTS), dtype=np.float32)
    starts, lengths, masks, at = [], [], [], 0
    for p in paths:
        x, y, _, _, mask = one(p)
        if len(x) > crop:
            k = int(rng.integers(0, len(x) - crop))
            x, y = x[k:k + crop], y[k:k + crop]
        big[at + pad:at + pad + len(x)] = x
        ys[at + pad:at + pad + len(x)] = y
        starts.append(at + pad)
        lengths.append(len(x))
        masks.append(mask)
        at += pad + len(x)
    return big[:at], ys[:at], np.array(starts), np.array(lengths), np.array(masks)


def sample(targets, starts, lengths, rng, negatives=3):
    """Every hit's hop, the hops around them, and random others (the melody learns on all)."""
    picks = []
    for k, (start, n) in enumerate(zip(starts, lengths)):
        y = targets[start:start + n]
        pos = np.flatnonzero(y[:, :3].max(axis=1) > 0)
        near = np.clip((pos[:, None] + np.arange(-8, 9)[None, :]).ravel(), 0, len(y) - 1)
        rand = rng.integers(0, len(y), size=max(len(pos), 200) * negatives)
        idx = np.unique(np.concatenate([pos, near, rand]))
        picks.append(np.stack([np.full(len(idx), k), idx], axis=1))
    return np.concatenate(picks)


def predict(model, x):
    view, slow = T.windows(x)
    out = []
    with torch.no_grad():
        for s in range(0, len(x), 16384):
            idx = np.arange(s, min(s + 16384, len(x)))
            out.append(torch.sigmoid(model(torch.from_numpy(T.inputs(view, slow, idx)).to(DEVICE))).cpu().numpy())
    return np.concatenate(out)


def rank(v):
    return np.argsort(np.argsort(v)).astype(np.float64)


def evaluate(model, tracks, thresholds):
    """Each drum's F (precision, recall) at these thresholds, and the melody's Spearman
    correlation with its target over the minutes that have one."""
    tot, corr = np.zeros((3, 3)), []
    for _, x, y, sr, hits, mask in tracks:
        p = predict(model, x.astype(np.float32))
        found = T.pick(p[:, :3], sr, thresholds)
        for c in range(3):
            tot[c] += T.score(list(hits[c]), found[c])
        if mask[3] and np.std(y[:, 3]) > 0.05:
            corr.append(np.corrcoef(rank(y[:, 3]), rank(p[:, 3]))[0, 1])
    res = []
    for m, f, t in tot:
        pr, rc = m / max(f, 1), m / max(t, 1)
        res.append((2 * pr * rc / max(pr + rc, 1e-9), pr, rc))
    return res, float(np.mean(corr)) if corr else float("nan")


def show(name, res, mel):
    print(f"{name}: " + "  ".join(f"{h} F {f:.3f} (p {p:.2f} r {r:.2f})" for h, (f, p, r) in zip(("kick", "snare", "hat"), res)) + f"  melody {mel:.2f}", flush=True)


if __name__ == "__main__":
    train_dirs, epochs, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    tests = sys.argv[4:]
    rng = np.random.default_rng(0)
    paths = sorted(p for d in train_dirs.split(",") for p in glob.glob(os.path.join(d, "*.npz")))
    rng.shuffle(paths)
    # tuning: a tenth of the real minutes (the ones with a melody target)
    real = [p for p in paths if "melody" in np.load(p, allow_pickle=True).files]
    held = set(real[: max(10, len(real) // 10)])
    big, targets, starts, lengths, masks = stacked([p for p in paths if p not in held], int(os.environ.get("CROP", 4000)), rng)
    val = load(sorted(held))
    print(f"{len(starts)} tracks to learn from ({len(big)} hops), {len(val)} real minutes to tune on", flush=True)
    model = Model().to(DEVICE)
    opt = torch.optim.Adam(model.parameters(), lr=1e-3, weight_decay=1e-5)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=max(1, epochs))
    loss_fn = nn.BCEWithLogitsLoss(reduction="none")
    best_state, best_f = None, -1.0
    for epoch in range(epochs):
        picks = sample(targets, starts, lengths, rng)
        rng.shuffle(picks)
        picks = picks[:3_000_000]
        model.train()
        started, total = time.time(), 0.0
        for s in range(0, len(picks), 8192):
            batch = picks[s:s + 8192]
            rows = starts[batch[:, 0]] + batch[:, 1]
            xb = torch.from_numpy(T.gather(big, rows)).to(DEVICE)
            yb = torch.from_numpy(targets[rows]).to(DEVICE)
            mb = torch.from_numpy(masks[batch[:, 0]]).to(DEVICE)
            xb = xb + 0.02 * torch.randn_like(xb)
            loss = (loss_fn(model(xb), yb) * mb).sum() / mb.sum().clamp(min=1.0)
            opt.zero_grad()
            loss.backward()
            opt.step()
            total += loss.item() * len(batch)
        sched.step()
        model.eval()
        res, mel = evaluate(model, val, (0.5, 0.5, 0.5))
        mean = sum(f for f, _, _ in res) / 3 + 0.3 * mel
        if mean > best_f:
            best_f, best_state = mean, {k: t.detach().clone() for k, t in model.state_dict().items()}
        show(f"epoch {epoch + 1} ({time.time() - started:.0f} s, loss {total / len(picks):.4f})", res, mel)
    model.load_state_dict(best_state)
    model.eval()
    best = []
    for c in range(3):
        scores = [(evaluate(model, val, [th if k == c else 0.5 for k in range(3)])[0][c][0], th) for th in np.arange(0.2, 0.9, 0.05)]
        best.append(round(float(max(scores)[1]), 2))
    print("thresholds", best, flush=True)
    torch.save(model.state_dict(), out + ".pt")
    T.export(model, out + ".bin")
    json.dump({"thresholds": best, "hidden": list(HIDDEN), "context": F.CONTEXT, "outputs": OUTPUTS}, open(out + ".json", "w"))
    for folder in tests:
        tracks = load(sorted(glob.glob(os.path.join(folder, "*.npz"))))
        show(os.path.basename(folder.rstrip("/\\")), *evaluate(model, tracks, best))
