"""Scores a trained drum model (.pt, with CONTEXT / HIDDEN / LOOKAHEAD as it was trained) on
eval folders: at its tuned thresholds, and each drum at its own best one."""
import glob, json, os, sys

import numpy as np
import torch

import drumtrain as T

path, folders = sys.argv[1], sys.argv[2:]
device = "mps" if torch.backends.mps.is_available() else "cpu"
model = T.Model().to(device)
model.load_state_dict(torch.load(path, map_location=device))
model.eval()
best = json.load(open(path[:-3] + ".json"))["thresholds"]
for folder in folders:
    tracks = T.load(sorted(glob.glob(os.path.join(folder, "*.npz"))))
    res = T.evaluate(model, tracks, device, best)
    tops = []
    for c in range(3):
        tops.append(max((T.evaluate(model, tracks, device, [th] * 3)[c][0], th) for th in np.arange(0.1, 0.9, 0.1)))
    print(f"{os.path.basename(folder):13s}", " ".join(f"{n} {f:.2f}" for n, (f, p, r) in zip(["kick", "snare", "hat"], res)), " | best:", " ".join(f"{n} {f:.2f}@{th:.1f}" for n, (f, th) in zip(["kick", "snare", "hat"], tops)), flush=True)
