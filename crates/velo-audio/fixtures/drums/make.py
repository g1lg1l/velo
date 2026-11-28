"""Remakes the fixtures drums.rs is tested with: random weights in the live model's shape
(drums-random.bin) and what they answer for drums.wav, hop by hop (drums-expected.bin), from the
features drumfeat_reference.py computes (drums-base.bin). Run it here after the model's shape
changes:

    python make.py <front> <state> <layers> <outputs>
"""
import sys
import wave

import numpy as np

import drumfeat_reference as F


def sigmoid(v):
    return 1 / (1 + np.exp(-v))


def answer(weights, base, state, layers):
    """The model's answer for every hop of `base`, from weights in drums.bin's order: the front
    layer over the last CONTEXT hops (zeros before the start), the GRUs, the output layer."""
    padded = np.concatenate([np.zeros((F.CONTEXT - 1, base.shape[1]), np.float32), base])
    h, out = [np.zeros(state, np.float32) for _ in range(layers)], []
    for t in range(len(base)):
        front = x = np.maximum(weights[0] @ padded[t:t + F.CONTEXT].ravel() + weights[1], 0)
        for l in range(layers):
            wi, wh, bi, bh = weights[2 + 4 * l:6 + 4 * l]
            a, b = wi @ x + bi, wh @ h[l] + bh
            r, z = sigmoid(a[:state] + b[:state]), sigmoid(a[state:2 * state] + b[state:2 * state])
            h[l] = x = (1 - z) * np.tanh(a[2 * state:] + r * b[2 * state:]) + z * h[l]
        out.append(sigmoid(weights[-2] @ np.concatenate([front, x]) + weights[-1]))
    return np.array(out)


def shapes(front, state, layers, outputs, base=192):
    gru = [s for l in range(layers) for s in [(3 * state, state if l else front), (3 * state, state), (3 * state,), (3 * state,)]]
    return [(front, F.CONTEXT * base), (front,)] + gru + [(outputs, front + state), (outputs,)]


if __name__ == "__main__":
    front, state, layers, outputs = (int(v) for v in sys.argv[1:5])
    w = wave.open("drums.wav")
    x = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0
    base = F.base(x, w.getframerate())
    rng = np.random.default_rng(7)
    weights = [(rng.standard_normal(s) * (0.5 / np.sqrt(s[-1]))).astype(np.float32) for s in shapes(front, state, layers, outputs)]
    base.astype("<f4").tofile("drums-base.bin")
    np.concatenate([a.ravel() for a in weights]).astype("<f4").tofile("drums-random.bin")
    answer(weights, base, state, layers).astype("<f4").tofile("drums-expected.bin")
    print(len(base), "hops,", sum(a.size for a in weights), "weights")
