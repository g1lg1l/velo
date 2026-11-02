"""The live model's material, as livetrain.py reads it: drumfeat's features of each real library
minute (how much of the sound is melody, from the melody stem; its hits are the teacher's, read
from the minute's labels.json when it trains, so labelling again needs no new features) and of
each generated clip (teacher.py make: every hit known, no melody target). Half of each,
picked by name, is resampled to 48 kHz first, the rate a computer's own output usually runs at.

    python livedata.py <library minutes> <out>      # a minute folder per real minute
    python livedata.py --clips <clips> <out>        # a clip*.flac with its .npz per clip
"""
import glob, hashlib, json, os, sys

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

import drumfeat as F

HITS = ("kick", "snare", "hat")
# the melody target: the melody stem's level against the mix's (150 Hz to 8 kHz, dB) mapped so a
# melody as loud as the rest is 1 and one 18 dB under it is 0; nothing in a silent mix
MELODY_FLOOR_DB, MELODY_SPAN_DB, SILENT_DB = -18.0, 15.0, -60.0


def rate_for(name):
    return 48000 if int(hashlib.sha1(name.encode()).hexdigest()[:4], 16) % 2 else 44100


def at_rate(x, sr, rate):
    return x if rate == sr else resample_poly(x, 160, 147).astype(np.float32)


def band_db(x, sr):
    """Each hop's level from 150 Hz to 8 kHz (dB), on drumfeat's frames."""
    db = F.frames_db(x, sr)
    mel = lambda f: 2595.0 * np.log10(1.0 + f / 700.0)
    centres = 700.0 * (10 ** (np.linspace(mel(F.FMIN), mel(min(F.FMAX, sr / 2)), F.BANDS + 2)[1:-1] / 2595.0) - 1.0)
    sel = (centres >= 150) & (centres <= 8000)
    return 10 * np.log10((10 ** (db[:, sel] / 10)).sum(axis=1) + 1e-10)


def melody_target(melody, mix, sr):
    m, x = band_db(melody, sr), band_db(mix, sr)
    t = np.clip((m - x - MELODY_FLOOR_DB) / MELODY_SPAN_DB, 0, 1)
    t[x < SILENT_DB] = 0.0
    return t.astype(np.float16)


def save(dest, x, sr, hits=None, melody=None, folder=None):
    out = dict(x=F.base(x, sr).astype(np.float16), sr=sr)
    if hits is not None:
        out["hits"] = np.array([np.array(h, dtype=np.float64) for h in hits], dtype=object)
    if melody is not None:
        out["melody"] = melody
    if folder is not None:
        out["folder"] = folder
    np.savez(dest, **out)


def minutes(root, out):
    os.makedirs(out, exist_ok=True)
    for name in sorted(os.listdir(root)):
        folder = os.path.join(root, name)
        dest = os.path.join(out, name + ".npz")
        if os.path.exists(dest) or not os.path.exists(os.path.join(folder, "meta.json")):
            continue
        mix, sr = sf.read(os.path.join(folder, "mix.flac"), dtype="float32")
        melody = sf.read(os.path.join(folder, "melody.flac"), dtype="float32")[0]
        rate = rate_for(name)
        mix, melody = at_rate(mix, sr, rate), at_rate(melody, sr, rate)
        save(dest, mix, rate, melody=melody_target(melody, mix, rate), folder=os.path.abspath(folder))
        print(name, rate, flush=True)


def clips(root, out):
    os.makedirs(out, exist_ok=True)
    for path in sorted(glob.glob(os.path.join(root, "clip*.npz"))):
        name = os.path.basename(path)[:-4]
        dest = os.path.join(out, name + ".npz")
        if os.path.exists(dest) or not os.path.exists(path[:-4] + ".flac"):
            continue
        d = np.load(path)
        x, sr = sf.read(path[:-4] + ".flac", dtype="float32")
        rate = rate_for(name)
        save(dest, at_rate(x, sr, rate), rate, [d[h] for h in HITS])
        print(name, rate, flush=True)


if __name__ == "__main__":
    if sys.argv[1] == "--clips":
        clips(sys.argv[2], sys.argv[3])
    else:
        minutes(sys.argv[1], sys.argv[2])
