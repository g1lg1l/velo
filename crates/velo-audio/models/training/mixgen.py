"""Training mixes with electronic drums: synthesized kicks, snares, claps and hats (808 and 909
style, every parameter drawn at random, one kit per mix) in patterns from four on the floor to
halftime with hat rolls, over Slakh's non-drum stems; every hit's time is exact. Toms and
percussion play too, unlabelled, so the model learns what isn't a kick, snare or hat. One .npz per
mix, as drumdata writes them."""
import glob, json, os, sys

import numpy as np
import soundfile as sf
import yaml
from scipy.signal import butter, fftconvolve, resample_poly, sosfilt

import drumfeat as F

SECONDS = 30.0


def env(n, sr, decay):
    return np.exp(-np.arange(n) / (decay * sr))


def band(x, sr, lo=None, hi=None, order=2):
    if lo and hi:
        sos = butter(order, [lo, min(hi, 0.45 * sr)], "bandpass", fs=sr, output="sos")
    elif lo:
        sos = butter(order, lo, "highpass", fs=sr, output="sos")
    else:
        sos = butter(order, min(hi, 0.45 * sr), "lowpass", fs=sr, output="sos")
    return sosfilt(sos, x)


def kick(r, sr):
    decay = r.uniform(0.08, 0.9)
    n = int(sr * min(5 * decay, 1.5))
    t = np.arange(n) / sr
    end = r.uniform(38, 70)
    f = end + end * r.uniform(1, 7) * np.exp(-t / r.uniform(0.005, 0.05))
    x = np.sin(2 * np.pi * np.cumsum(f) / sr) * env(n, sr, decay)
    # the click: the beater, a 909's pulse, or plain noise; often as loud as the body, and as
    # short as a closed hat (a kick's click is not a hat)
    if r.random() < 0.8:
        m = int(sr * r.uniform(0.001, 0.01))
        kind = r.integers(4)
        if kind == 0:
            click = r.standard_normal(m)
        elif kind == 1:
            click = band(r.standard_normal(m), sr, lo=r.uniform(1000, 4000))
        elif kind == 2:
            c = r.uniform(2000, 6000)
            click = band(r.standard_normal(m), sr, c / 2, c * 2)
        else:
            click = np.sin(2 * np.pi * r.uniform(1000, 3500) * np.arange(m) / sr)
        x[:m] += r.uniform(0.1, 1.0) * click / (np.abs(click).max() + 1e-9) * env(m, sr, r.uniform(0.001, 0.004))
    if r.random() < 0.5:
        g = r.uniform(1, 6)
        x = np.tanh(g * x) / np.tanh(g)
    return x


def snare(r, sr):
    n = int(sr * 0.6)
    t = np.arange(n) / sr
    f = r.uniform(140, 280)
    tone = (np.sin(2 * np.pi * f * t) + 0.5 * np.sin(2 * np.pi * f * r.uniform(1.5, 1.9) * t)) * env(n, sr, r.uniform(0.03, 0.15))
    noise = band(r.standard_normal(n), sr, r.uniform(500, 2000), r.uniform(4000, 12000)) * env(n, sr, r.uniform(0.06, 0.35))
    return r.uniform(0.1, 1) * tone + r.uniform(0.6, 1.4) * noise / (np.abs(noise).max() + 1e-9)


def clap(r, sr):
    n = int(sr * 0.7)
    x = np.zeros(n)
    at = 0
    for _ in range(r.integers(2, 6)):
        m = int(sr * 0.03)
        x[at:at + m] += r.standard_normal(m) * env(m, sr, r.uniform(0.003, 0.008))
        at += int(sr * r.uniform(0.006, 0.014))
    x[at:] += r.standard_normal(n - at) * env(n - at, sr, r.uniform(0.08, 0.4)) * r.uniform(0.3, 0.8)
    c = r.uniform(800, 2500)
    return band(x, sr, c / r.uniform(1.5, 3), c * r.uniform(1.5, 4))


def hat(r, sr, open_):
    decay = r.uniform(0.15, 0.6) if open_ else r.uniform(0.015, 0.08)
    n = int(sr * min(5 * decay, 1.0))
    t = np.arange(n) / sr
    if r.random() < 0.5:
        base = r.uniform(250, 500)
        x = sum(np.sign(np.sin(2 * np.pi * base * k * t)) for k in (2, 3, 4.16, 5.43, 6.79, 8.21))
    else:
        x = r.standard_normal(n)
    return band(x, sr, lo=r.uniform(5000, 10000), order=4) * env(n, sr, decay)


def tom(r, sr):
    n = int(sr * 0.5)
    t = np.arange(n) / sr
    f = r.uniform(80, 250) * (1 + 0.5 * np.exp(-t / 0.03))
    return np.sin(2 * np.pi * np.cumsum(f) / sr) * env(n, sr, r.uniform(0.1, 0.4))


def perc(r, sr):
    n = int(sr * 0.2)
    t = np.arange(n) / sr
    f = r.uniform(300, 2000)
    return (np.sin(2 * np.pi * f * t) + 0.3 * np.sin(2 * np.pi * 1.48 * f * t)) * env(n, sr, r.uniform(0.02, 0.08))


def norm(x):
    return x / (np.sqrt(np.mean(x[: max(1, len(x) // 4)] ** 2)) + 1e-9)


def kit(r, sr):
    k = {"kick": norm(kick(r, sr)), "hat": norm(hat(r, sr, False)), "open": norm(hat(r, sr, True)), "tom": norm(tom(r, sr)), "perc": norm(perc(r, sr))}
    kind = r.choice(["snare", "clap", "both"])
    s = snare(r, sr) if kind != "clap" else clap(r, sr)
    if kind == "both":
        c = clap(r, sr)
        s = np.pad(s, (0, max(0, len(c) - len(s))))
        s[: len(c)] += c
    k["snare"] = norm(s)
    return k


def pattern(r, bpm):
    """(step in 16ths from the start, voice, velocity) for the whole mix; hat rolls go in 32nds
    and triplets, as fractional steps."""
    style = r.choice(["four", "break", "half", "dnb", "random"])
    if style == "dnb" and bpm < 140:
        bpm = r.uniform(160, 180)
    if style == "half" and bpm < 120:
        bpm = r.uniform(130, 150)
    steps = 16 * int(SECONDS * bpm / 60 / 4 + 1)
    rand = {v: r.uniform(0.05, 0.4) for v in ("kick", "snare", "hat")}
    # where the hats go: the off-beats only (a clicky kick on the beat with no hat there), 8ths,
    # or 16ths
    hats = r.choice(["off", "eighths", "sixteenths"], p=[0.4, 0.3, 0.3])
    hats16 = hats == "sixteenths"
    hat_on = lambda s: s % 4 == 2 if hats == "off" else (s % 2 == 0 or (hats16 and r.random() < 0.7))
    hits = []
    for bar in range(steps // 16):
        phrase = bar // 4
        rs = np.random.default_rng(r.integers(1 << 30) if r.random() < 0.3 else phrase)
        drop = {v: rs.random() < 0.12 for v in ("kick", "snare", "hat")}
        b = bar * 16
        for s in range(16):
            on = {"kick": False, "snare": False, "hat": False, "open": False}
            if style == "four":
                on["kick"] = s % 4 == 0
                on["snare"] = s in (4, 12)
                on["hat"] = hat_on(s)
                on["open"] = s % 4 == 2 and r.random() < 0.5
            elif style == "break":
                on["kick"] = s in (0, 10) or (s in (6, 7, 11) and r.random() < 0.3)
                on["snare"] = s in (4, 12) or (s in (7, 9, 15) and r.random() < 0.15)
                on["hat"] = hat_on(s)
            elif style == "half":
                on["kick"] = s == 0 or (s in (3, 7, 10, 11, 14) and r.random() < 0.25)
                on["snare"] = s == 8
                on["hat"] = hat_on(s)
            elif style == "dnb":
                on["kick"] = s in (0, 10) or (s == 13 and r.random() < 0.3)
                on["snare"] = s in (4, 12)
                on["hat"] = hat_on(s)
            else:
                for v in ("kick", "snare", "hat"):
                    on[v] = r.random() < rand[v] * (2 if s % 4 == 0 and v == "kick" else 1)
            if bar % 4 == 3 and s >= 8 and r.random() < 0.3:
                on["snare"] = True  # a fill
            for v, yes in on.items():
                if yes and not drop.get(v if v != "open" else "hat"):
                    hits.append((b + s, v, r.uniform(0.6, 1.0) if v != "hat" or s % 2 == 0 else r.uniform(0.3, 0.8)))
            if r.random() < 0.03:
                hits.append((b + s, r.choice(["tom", "perc"]), r.uniform(0.4, 1.0)))
            if style in ("half", "four", "random") and r.random() < 0.04 and not drop["hat"]:
                k = r.choice([2, 3, 4])
                hits += [(b + s + i / k, "hat", r.uniform(0.3, 0.9)) for i in range(k)]
    return bpm, hits


def stems_index(slakh):
    """Each track's non-drum stems that were rendered."""
    out = []
    for meta in glob.glob(os.path.join(slakh, "**", "metadata.yaml"), recursive=True):
        d = os.path.dirname(meta)
        m = yaml.safe_load(open(meta))
        files = [os.path.join(d, "stems", s + ".flac") for s, v in (m.get("stems") or {}).items() if not v.get("is_drum") and v.get("audio_rendered")]
        files = [f for f in files if os.path.exists(f)]
        if files:
            out.append(files)
    return out


def backing(r, index, sr, n):
    x = np.zeros(n)
    if not index or r.random() < 0.08:
        return x
    for track in [index[r.integers(len(index))] for _ in range(1 if r.random() < 0.7 else 2)]:
        for f in r.choice(track, size=min(len(track), r.integers(1, 6)), replace=False):
            try:
                info = sf.info(f)
                start = r.integers(0, max(1, info.frames - int(n * info.samplerate / sr)))
                s, ssr = sf.read(f, start=start, frames=int(n * info.samplerate / sr) + 10, always_2d=True, dtype="float32")
            except Exception:
                continue
            s = s.mean(axis=1)
            if ssr != sr:
                s = resample_poly(s, 160, 147) if (ssr, sr) == (44100, 48000) else resample_poly(s, sr, ssr)
            s = s[:n]
            x[: len(s)] += s * 10 ** (r.uniform(-12, 0) / 20)
    return x


def mix(seed, index):
    r = np.random.default_rng(seed)
    sr = int(r.choice([44100, 48000]))
    n = int(SECONDS * sr)
    bpm, hits = pattern(r, r.uniform(70, 180))
    k = kit(r, sr)
    gains = {"kick": r.uniform(-6, 0), "snare": r.uniform(-10, 0), "hat": r.uniform(-24, -6), "open": r.uniform(-26, -8), "tom": r.uniform(-12, -3), "perc": r.uniform(-18, -6)}
    swing = r.uniform(0, 0.15) if r.random() < 0.4 else 0.0
    sixteenth = 60 / bpm / 4
    drums, kicks_env = np.zeros(n + sr * 2), np.zeros(n + sr * 2)
    labels = [[], [], []]
    cls = {"kick": 0, "snare": 1, "hat": 2, "open": 2}
    for step, v, vel in hits:
        t = (step + (swing if int(step) % 2 == 1 and step == int(step) else 0)) * sixteenth + r.normal(0, 0.002)
        i = int(max(0, t) * sr)
        if i >= n:
            continue
        sound = k[v] * vel * 10 ** (gains[v] / 20)
        drums[i:i + len(sound)] += sound
        if v == "kick":
            kicks_env[i:i + int(0.15 * sr)] = np.maximum(kicks_env[i:i + int(0.15 * sr)], np.linspace(1, 0, int(0.15 * sr)))
        if v in cls:
            labels[cls[v]].append(i / sr)
    drums, kicks_env = drums[:n], kicks_env[:n]
    if r.random() < 0.4:
        tail = int(sr * r.uniform(0.2, 1.2))
        ir = r.standard_normal(tail) * env(tail, sr, r.uniform(0.05, 0.3))
        wet = fftconvolve(drums, ir / np.sqrt(np.sum(ir ** 2)))[:n]
        drums = drums + r.uniform(0.05, 0.3) * wet
    back = backing(r, index, sr, n)
    if r.random() < 0.4:
        back *= 1 - r.uniform(0.3, 0.8) * kicks_env
    rms = lambda a: np.sqrt(np.mean(a ** 2)) + 1e-9
    if back.any():
        back *= rms(drums) / rms(back) * 10 ** (r.uniform(-12, 6) / 20)
    x = drums + back
    if r.random() < 0.3:
        g = r.uniform(1, 4)
        x = np.tanh(g * x / np.abs(x).max()) / np.tanh(g)
    x = x / (np.abs(x).max() + 1e-9) * 10 ** (r.uniform(-12, -1) / 20)
    return x.astype(np.float32), sr, [sorted(l) for l in labels], bpm


def build(seed, index, out):
    dest = os.path.join(out, f"mix{seed:05d}.npz")
    if os.path.exists(dest):
        return
    x, sr, hits, bpm = mix(seed, index)
    b = F.base(x, sr)
    np.savez(dest, x=b.astype(np.float16), y=np.zeros((len(b), 3), dtype=np.float32), sr=sr, hits=np.array([np.array(h) for h in hits], dtype=object), exact=1)
    print(dest, sr, round(bpm), [len(h) for h in hits], flush=True)


if __name__ == "__main__":
    slakh, out, count = sys.argv[1], sys.argv[2], int(sys.argv[3])
    os.makedirs(out, exist_ok=True)
    index = stems_index(slakh) if slakh != "-" else []
    print(len(index), "tracks of stems", flush=True)
    if len(sys.argv) > 4:  # a WAV to listen to
        x, sr, hits, bpm = mix(int(sys.argv[4]), index)
        sf.write(os.path.join(out, f"mix{sys.argv[4]}.wav"), x, sr)
        json.dump(dict(zip(["kick", "snare", "hat"], hits)), open(os.path.join(out, f"mix{sys.argv[4]}.json"), "w"))
        print(bpm, [len(h) for h in hits])
        sys.exit()
    from multiprocessing import Pool
    with Pool(8) as pool:
        pool.starmap(build, [(s, index, out) for s in range(count)])
