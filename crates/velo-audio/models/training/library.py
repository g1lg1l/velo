"""Turns a music library into training material: a minute from the middle of each track (a few,
spread out, for a long set), split by Demucs (htdemucs) into the drums, the bass and the rest
(the melody: Demucs' other and vocals), saved as mono 44.1 kHz FLAC next to the whole mix. One
folder per minute; a tenth of the tracks, picked by their path's hash, go to test/ and never
train anything.

    python library.py <library> <out>
"""
import hashlib, json, os, subprocess, sys, time

import numpy as np
import soundfile as sf
import torch
from demucs.apply import apply_model
from demucs.pretrained import get_model

SR = 44100
CROP_S = 60.0
AUDIO = {".mp3", ".flac", ".wav", ".m4a", ".aiff", ".aif"}
# not music: samples, effects, recordings of sets
SKIP = ("DJ Tools", "Sampler", "Recording")


def key(rel):
    return hashlib.sha1(rel.replace("\\", "/").encode("utf-8")).hexdigest()


def duration(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path], capture_output=True, text=True)
    try:
        return float(out.stdout.strip())
    except ValueError:
        return 0.0


def decode(path, start, seconds):
    """Stereo float samples at SR from `start` for `seconds`."""
    cmd = ["ffmpeg", "-v", "error", "-ss", f"{start:.3f}", "-t", f"{seconds:.3f}", "-i", path, "-f", "f32le", "-ac", "2", "-ar", str(SR), "-"]
    raw = subprocess.run(cmd, capture_output=True).stdout
    return np.frombuffer(raw, dtype=np.float32).reshape(-1, 2).T.copy()


def crops(rel, length):
    """Where each minute starts: the middle half of a track, or every ten minutes of a set."""
    rng = np.random.default_rng(int(key(rel)[:8], 16))
    if length < CROP_S + 5:
        return []
    if length < 900:
        return [rng.uniform(0.25, 0.7) * (length - CROP_S)]
    count = min(6, int(length // 600))
    return [(k + rng.uniform(0.2, 0.8)) * (length - CROP_S) / count for k in range(count)]


def separate(model, device, audio):
    """Stereo samples at SR split into mono drums, bass and melody (Demucs' other and vocals)."""
    wav = torch.from_numpy(audio).to(device)
    ref = wav.mean(0)
    mean, std = ref.mean(), ref.std() + 1e-8
    with torch.no_grad():
        stems = apply_model(model, ((wav - mean) / std)[None], shifts=0, split=True, overlap=0.25, progress=False)[0]
    stems = (stems * std + mean).cpu().numpy()
    names = list(model.sources)
    return {
        "drums": stems[names.index("drums")].mean(0),
        "bass": stems[names.index("bass")].mean(0),
        "melody": (stems[names.index("other")] + stems[names.index("vocals")]).mean(0),
    }


def demucs():
    model = get_model("htdemucs")
    device = "cuda" if torch.cuda.is_available() else "cpu"
    return model.to(device).eval(), device


def main(library, out):
    model, device = demucs()
    tracks = []
    for root, _, files in os.walk(library):
        if any(s in root for s in SKIP):
            continue
        tracks += [os.path.join(root, f) for f in files if os.path.splitext(f)[1].lower() in AUDIO]
    # in no order: whatever is split so far spans the whole library
    tracks.sort()
    np.random.default_rng(0).shuffle(tracks)
    print(len(tracks), "tracks", flush=True)
    started = time.time()
    for n, path in enumerate(tracks):
        rel = os.path.relpath(path, library)
        part = "test" if int(key(rel)[:8], 16) % 10 == 0 else "train"
        for k, start in enumerate(crops(rel, duration(path))):
            dest = os.path.join(out, part, f"{key(rel)[:12]}-{k}")
            if os.path.exists(os.path.join(dest, "meta.json")):
                continue
            audio = decode(path, start, CROP_S)
            if audio.shape[1] < SR * 10 or np.sqrt(np.mean(audio ** 2)) < 1e-3:
                continue
            mono = {"mix": audio.mean(0), **separate(model, device, audio)}
            peak = max(np.abs(x).max() for x in mono.values())
            scale = 0.99 / peak if peak > 0.99 else 1.0
            os.makedirs(dest, exist_ok=True)
            for name, x in mono.items():
                sf.write(os.path.join(dest, name + ".flac"), (x * scale).astype(np.float32), SR, subtype="PCM_24")
            folder = rel.replace("\\", "/").split("/")
            json.dump({"track": rel, "start": round(start, 3), "seconds": audio.shape[1] / SR, "folder": folder[1] if len(folder) > 2 else ""}, open(os.path.join(dest, "meta.json"), "w"))
        if n % 20 == 0:
            print(f"{n + 1}/{len(tracks)} tracks, {time.time() - started:.0f} s", flush=True)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
