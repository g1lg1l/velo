"""Remakes the fixtures drums.rs is tested with: random weights in the live model's shape
(drums-random.bin) and what they answer for drums.wav, hop by hop (drums-expected.bin), from the
features drumfeat_reference.py computes (drums-base.bin). Run it here after the model's shape
changes:

    python make.py <hidden0> <hidden1> <outputs>
"""
import sys
import wave

import numpy as np

import drumfeat_reference as F

hidden0, hidden1, outputs = (int(v) for v in sys.argv[1:4])
w = wave.open("drums.wav")
x = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0
base = F.base(x, w.getframerate())
rng = np.random.default_rng(7)
shapes = [(hidden0, F.INPUTS), (hidden0,), (hidden1, hidden0), (hidden1,), (outputs, hidden1), (outputs,)]
weights = [(rng.standard_normal(s) * (0.5 / np.sqrt(s[-1]))).astype(np.float32) for s in shapes]
inputs = np.stack([F.stack(base, t) for t in range(len(base))])
h = np.maximum(inputs @ weights[0].T + weights[1], 0)
h = np.maximum(h @ weights[2].T + weights[3], 0)
out = 1 / (1 + np.exp(-(h @ weights[4].T + weights[5])))
base.astype("<f4").tofile("drums-base.bin")
np.concatenate([a.ravel() for a in weights]).astype("<f4").tofile("drums-random.bin")
out.astype("<f4").tofile("drums-expected.bin")
print(len(base), "hops,", sum(a.size for a in weights), "weights")
