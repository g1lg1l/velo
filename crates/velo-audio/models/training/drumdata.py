"""Training and evaluation data for the drum model: each track's features (drumfeat.base) and its
hits as frame targets. Slakh tracks: the mix and the drum stems' MIDI notes (General MIDI kit
numbers); eval folders: a WAV and its JSON of kick / snare / hat times."""
import glob, json, os, struct, sys

import numpy as np
import soundfile as sf
import yaml

import drumfeat as F

KIT = {35: 0, 36: 0, 37: 1, 38: 1, 39: 1, 40: 1, 42: 2, 44: 2, 46: 2}  # kick, snare/clap, hat


def midi_notes(path):
    """(seconds, note) of every note-on in a standard MIDI file, through its tempo map."""
    data = open(path, "rb").read()
    if data[:4] != b"MThd":
        return []
    _, ntrks, division = struct.unpack(">HHH", data[8:14])
    at, events, tempos = 14, [], [(0, 500000)]
    for _ in range(ntrks):
        if data[at:at + 4] != b"MTrk":
            break
        length = struct.unpack(">I", data[at + 4:at + 8])[0]
        i, end, tick, status = at + 8, at + 8 + length, 0, 0
        while i < end:
            delta = 0
            while True:
                b = data[i]; i += 1
                delta = (delta << 7) | (b & 0x7F)
                if b < 0x80:
                    break
            tick += delta
            b = data[i]
            if b == 0xFF:
                kind, i = data[i + 1], i + 2
                n = 0
                while True:
                    c = data[i]; i += 1
                    n = (n << 7) | (c & 0x7F)
                    if c < 0x80:
                        break
                if kind == 0x51:
                    tempos.append((tick, int.from_bytes(data[i:i + 3], "big")))
                i += n
                continue
            if b in (0xF0, 0xF7):
                i += 1
                n = 0
                while True:
                    c = data[i]; i += 1
                    n = (n << 7) | (c & 0x7F)
                    if c < 0x80:
                        break
                i += n
                continue
            if b & 0x80:
                status, i = b, i + 1
            kind = status & 0xF0
            if kind in (0xC0, 0xD0):
                i += 1
                continue
            a, v = data[i], data[i + 1]
            i += 2
            if kind == 0x90 and v > 0:
                events.append((tick, a, status & 0x0F))
        at = end
    tempos.sort()

    def seconds(tick):
        s, last_tick, us = 0.0, 0, 500000
        for t, tempo in tempos:
            if t > tick:
                break
            s += (t - last_tick) * us / division / 1e6
            last_tick, us = t, tempo
        return s + (tick - last_tick) * us / division / 1e6

    return [(seconds(t), n, ch) for t, n, ch in events]


def slakh_hits(track):
    """The kit's hits in the track's whole MIDI (all_src.mid): General MIDI drums are on channel
    10 (the stems' own files have every part on channel 1)."""
    hits = [[], [], []]
    for mid in glob.glob(os.path.join(track, "all_src.mid")):
        for t, n, ch in midi_notes(mid):
            if ch == 9 and n in KIT:
                hits[KIT[n]].append(t)
    return [sorted(set(round(t, 4) for t in h)) for h in hits]


def targets(hits, n_frames, sr):
    """Per frame and class: 1 on the frame whose hop holds the hit and the next one."""
    y = np.zeros((n_frames, 3), dtype=np.float32)
    for c, times in enumerate(hits):
        for t in times:
            f = int(t * sr // F.HOP)
            if 0 <= f < n_frames:
                y[f, c] = 1.0
            if 0 <= f + 1 < n_frames:
                y[f + 1, c] = 1.0
    return y


def load_mono(path):
    x, sr = sf.read(path, always_2d=True, dtype="float32")
    return x.mean(axis=1), sr


def build(kind, item, out):
    """One track's .npz: base features (float16), frame targets, sample rate, hits (a file that
    won't decode, a track cut short by the download, is skipped)."""
    try:
        build_one(kind, item, out)
    except Exception as e:
        print("skipped", item, type(e).__name__, flush=True)


def build_one(kind, item, out):
    if True:
        name = os.path.basename(item.rstrip("/"))
        dest = os.path.join(out, name.replace(".wav", "") + ".npz")
        if os.path.exists(dest):
            return
        if kind == "slakh":
            x, sr = load_mono(os.path.join(item, "mix.flac"))
            hits = slakh_hits(item)
            if sum(len(h) for h in hits) < 20:
                return
            # a third of them at 48 kHz, as a computer's own output usually is
            if int(name[-3:]) % 3 == 0 and sr == 44100:
                from scipy.signal import resample_poly
                x, sr = resample_poly(x, 160, 147).astype(np.float32), 48000
        else:
            x, sr = load_mono(item)
            j = json.load(open(item[:-4] + ".json"))
            hits = [j["kick"], j["snare"], j["hat"]]
        b = F.base(x, sr)
        y = targets(hits, len(b), sr)
        np.savez(dest, x=b.astype(np.float16), y=y, sr=sr, hits=np.array([np.array(h) for h in hits], dtype=object))
        print(name, sr, len(b), [len(h) for h in hits], flush=True)


if __name__ == "__main__":
    # build: one .npz per track with base features (float16) and targets
    kind, src, out = sys.argv[1:4]
    os.makedirs(out, exist_ok=True)
    if kind == "slakh":
        items = sorted(os.path.dirname(p) for p in glob.glob(os.path.join(src, "**", "mix.flac"), recursive=True))
    else:
        items = sorted(glob.glob(os.path.join(src, "*.wav")))
    from multiprocessing import Pool
    with Pool(8) as pool:
        pool.starmap(build, [(kind, item, out) for item in items])
