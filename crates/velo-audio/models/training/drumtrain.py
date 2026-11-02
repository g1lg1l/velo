"""Trains the drum model (a small MLP over drumfeat's inputs) on Slakh tracks and scores it, per
class, on held-out folders with exact labels. Saves weights as little-endian f32 for velo-audio."""
import glob, json, os, sys, time

import numpy as np
import torch
from torch import nn

import drumfeat as F

HIDDEN = tuple(int(h) for h in os.environ.get("HIDDEN", "256,128").split(","))
# hops the model waits before it decides: it sees the whole attack, that much later
LOOKAHEAD = int(os.environ.get("LOOKAHEAD", 3))
REFRACTORY_S = (0.06, 0.06, 0.04)


class Model(nn.Module):
    def __init__(self):
        super().__init__()
        a, b = HIDDEN
        self.net = nn.Sequential(nn.Linear(F.INPUTS, a), nn.ReLU(), nn.Dropout(0.1), nn.Linear(a, b), nn.ReLU(), nn.Linear(b, 3))

    def forward(self, x):
        return self.net(x)


def windows(x):
    """(n, INPUTS) inputs for every hop of a track's base features, built lazily per batch."""
    pad = np.concatenate([np.zeros((F.CONTEXT - 1, x.shape[1]), dtype=x.dtype), x])
    view = np.lib.stride_tricks.sliding_window_view(pad[:, :128], (F.CONTEXT, 128))[:, 0]
    return view, x[:, 128:]


def inputs(view, slow, idx):
    return np.concatenate([view[idx].reshape(len(idx), -1), slow[idx]], axis=1).astype(np.float32)


# where each drum's hits show: the mel bands whose rise they should lift
SHOWS = (slice(0, 10), slice(20, 45), slice(48, 64))


def trusted(x, hits, sr):
    """Which drums' labels are real: Slakh's MIDI has parts its mix doesn't play, and a label
    on nothing would teach the model to hear what isn't there. A drum's labels count when its
    bands rise on them twice as much as on average."""
    hps = sr / F.HOP
    ok = np.zeros(3, dtype=np.float32)
    for c in range(3):
        h = np.asarray(hits[c])
        if len(h) < 10:
            continue
        rise = x[:, 64:128][:, SHOWS[c]].astype(np.float32).sum(axis=1)
        idx = (h * hps).astype(int)
        at = max(rise[np.clip(idx + lag, 0, len(rise) - 1)].mean() for lag in range(5))
        ok[c] = float(at >= 2.0 * rise.mean())
    return ok


def snapped(x, hits, sr):
    """Targets where each hit really starts: MIDI and the rendered audio drift apart by a few
    milliseconds from track to track, so each label moves to the hop, within -2..+4 of it, where
    its drum's bands rise most; that hop is 1, its neighbours 0.5."""
    hps = sr / F.HOP
    y = np.zeros((len(x), 3), dtype=np.float32)
    for c in range(3):
        rise = x[:, 64:128][:, SHOWS[c]].astype(np.float32).sum(axis=1)
        for t in hits[c]:
            f = int(t * hps)
            lo, hi = max(0, f - 2), min(len(x), f + 5)
            if lo >= hi:
                continue
            g = lo + int(np.argmax(rise[lo:hi])) + LOOKAHEAD
            if g >= len(x):
                continue
            y[g, c] = 1.0
            for n in (g - 1, g + 1):
                if 0 <= n < len(x):
                    y[n, c] = max(y[n, c], 0.5)
    return y


def load(paths, check=False, crop=None, rng=None):
    """Each track's features, targets and labels; with `crop`, a random stretch of that many hops
    of each (variety matters more than length, and memory runs out)."""
    tracks = []
    for p in paths:
        d = np.load(p, allow_pickle=True)
        x, y, sr, hits = d["x"], d["y"], int(d["sr"]), d["hits"]
        # generated mixes (exact) play every hit they label
        exact = "exact" in d.files
        if check or exact:
            y = snapped(x, hits, sr)
        mask = trusted(x, hits, sr) if check and not exact else np.ones(3, dtype=np.float32)
        if mask.sum() == 0:
            continue
        if crop and len(x) > crop:
            at = int(rng.integers(0, len(x) - crop))
            x, y = x[at:at + crop], y[at:at + crop]
        tracks.append((p, x, y, sr, hits, mask))
    return tracks


def stacked(tracks):
    """Every track's base features end to end, each after CONTEXT - 1 silent hops (so any hop's
    window is one gather), its targets alongside, and the row each track starts at."""
    xs, ys, starts, at = [], [], [], 0
    for _, x, y, _, _, _ in tracks:
        pad = F.CONTEXT - 1
        xs += [np.zeros((pad, x.shape[1]), np.float16), x.astype(np.float16)]
        ys += [np.zeros((pad, 3), np.float32), y]
        starts.append(at + pad)
        at += pad + len(x)
    return np.concatenate(xs), np.concatenate(ys), np.array(starts)


def gather(big, rows):
    w = big[rows[:, None] + np.arange(1 - F.CONTEXT, 1)[None, :], :128].reshape(len(rows), -1)
    return np.concatenate([w, big[rows, 128:]], axis=1).astype(np.float32)


def sample(tracks, rng, negatives=3):
    """Every positive hop, the hops around them (the hard negatives), and random others."""
    picks = []
    for k, (_, x, y, _, _, _) in enumerate(tracks):
        pos = np.flatnonzero(y.max(axis=1) > 0)
        near = np.clip((pos[:, None] + np.arange(-8, 9)[None, :]).ravel(), 0, len(y) - 1)
        rand = rng.integers(0, len(y), size=len(pos) * negatives)
        idx = np.unique(np.concatenate([pos, near, rand]))
        picks.append(np.stack([np.full(len(idx), k), idx], axis=1))
    return np.concatenate(picks)


def predict(model, x, device):
    view, slow = windows(x)
    out = []
    with torch.no_grad():
        for s in range(0, len(x), 8192):
            idx = np.arange(s, min(s + 8192, len(x)))
            out.append(torch.sigmoid(model(torch.from_numpy(inputs(view, slow, idx)).to(device))).cpu().numpy())
    return np.concatenate(out)


def pick(p, sr, thresholds):
    """Hits from probabilities: rising over the threshold, then quiet for the refractory time."""
    hop_s = F.HOP / sr
    found = [[], [], []]
    for c in range(3):
        last = -1e9
        for t in range(1, len(p)):
            if p[t, c] >= thresholds[c] and p[t - 1, c] < thresholds[c] and t * hop_s - last >= REFRACTORY_S[c]:
                found[c].append(t * hop_s)
                last = t * hop_s
    return found


def score(truth, found, tol=0.03):
    used, m = set(), 0
    found = np.array(found)
    for t in truth:
        if len(found) == 0:
            break
        d = np.abs(found - t)
        d[list(used)] = 9
        j = int(np.argmin(d))
        if d[j] <= tol:
            used.add(j)
            m += 1
    return m, len(found), len(truth)


def evaluate(model, tracks, device, thresholds, tol=0.03):
    tot = np.zeros((3, 3))
    for _, x, _, sr, hits, mask in tracks:
        found = pick(predict(model, x.astype(np.float32), device), sr, thresholds)
        for c in range(3):
            if mask[c]:
                tot[c] += score(list(hits[c]), found[c], tol)
    res = []
    for c in range(3):
        m, f, t = tot[c]
        p, r = m / max(f, 1), m / max(t, 1)
        res.append((2 * p * r / max(p + r, 1e-9), p, r))
    return res


def export(model, path):
    params = [t.detach().cpu().numpy().astype("<f4") for t in model.state_dict().values()]
    with open(path, "wb") as fh:
        for a in params:
            fh.write(a.tobytes())
    print("exported", sum(a.size for a in params), "weights to", path)


if __name__ == "__main__":
    # training folders joined by commas
    train_dirs, epochs, out = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    evals = sys.argv[4:]
    device = "mps" if torch.backends.mps.is_available() else "cpu"
    rng = np.random.default_rng(0)
    paths = sorted(p for d in train_dirs.split(",") for p in glob.glob(os.path.join(d, "*.npz")))
    rng.shuffle(paths)
    held = max(4, len(paths) // 10)
    train, val = load(paths[held:], check=True, crop=int(os.environ.get("CROP", 16000)), rng=rng), load(paths[:held], check=True)
    print(f"{len(train)} train tracks, {len(val)} validation", flush=True)
    model = Model().to(device)
    opt = torch.optim.Adam(model.parameters(), lr=1e-3, weight_decay=1e-5)
    sched = torch.optim.lr_scheduler.CosineAnnealingLR(opt, T_max=max(1, epochs))
    best_state, best_f = None, -1.0
    loss_fn = nn.BCEWithLogitsLoss(reduction="none")
    big, targets, starts = stacked(train)
    masks = np.array([t[5] for t in train])
    # the stack holds them now
    train = [(p, None, y, sr, None, m) for p, _, y, sr, _, m in train]
    for epoch in range(epochs):
        picks = sample(train, rng)
        rng.shuffle(picks)
        picks = picks[:2_000_000]
        model.train()
        started, total = time.time(), 0.0
        for s in range(0, len(picks), 4096):
            batch = picks[s:s + 4096]
            rows = starts[batch[:, 0]] + batch[:, 1]
            xb = torch.from_numpy(gather(big, rows)).to(device)
            yb = torch.from_numpy(targets[rows]).to(device)
            mb = torch.from_numpy(masks[batch[:, 0]]).to(device)
            # a little noise: no two mixes sound quite alike
            xb = xb + 0.02 * torch.randn_like(xb)
            loss = (loss_fn(model(xb), yb) * mb).sum() / mb.sum().clamp(min=1.0)
            opt.zero_grad()
            loss.backward()
            opt.step()
            total += loss.item() * len(batch)
        sched.step()
        model.eval()
        v = evaluate(model, val, device, (0.5, 0.5, 0.5))
        mean_f = sum(x[0] for x in v) / 3
        if mean_f > best_f:
            best_f, best_state = mean_f, {k: t.detach().clone() for k, t in model.state_dict().items()}
        print(f"epoch {epoch + 1}: loss {total / len(picks):.4f}, {len(picks)} samples, {time.time() - started:.0f} s; validation F kick {v[0][0]:.2f} snare {v[1][0]:.2f} hat {v[2][0]:.2f}", flush=True)
    model.load_state_dict(best_state)
    # thresholds tuned on validation
    best = []
    for c in range(3):
        scores = []
        for th in np.arange(0.2, 0.9, 0.05):
            ths = [0.5, 0.5, 0.5]
            ths[c] = th
            scores.append((evaluate(model, val, device, ths)[c][0], th))
        best.append(max(scores)[1])
    print("thresholds", [round(b, 2) for b in best], flush=True)
    torch.save(model.state_dict(), out + ".pt")
    export(model, out + ".bin")
    json.dump({"thresholds": [float(b) for b in best]}, open(out + ".json", "w"))
    for folder in evals:
        tracks = load(sorted(glob.glob(os.path.join(folder, "*.npz"))))
        res = evaluate(model, tracks, device, best)
        print(folder, " ".join(f"{n} F {f:.2f} (p {p:.2f} r {r:.2f})" for n, (f, p, r) in zip(["kick", "snare", "hat"], res)), flush=True)
        # how good each drum could be there, at its own best threshold
        tops = []
        for c in range(3):
            fs = [(evaluate(model, tracks, device, [th] * 3)[c][0], th) for th in np.arange(0.1, 0.9, 0.1)]
            tops.append(max(fs))
        print("  at their best:", " ".join(f"{n} F {f:.2f} (th {th:.1f})" for n, (f, th) in zip(["kick", "snare", "hat"], tops)), flush=True)
