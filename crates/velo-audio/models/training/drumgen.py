"""Generated electronic drums with every hit known, over the bass and melody of a real minute of
the library (library.py): what the labelling teacher and the live model learn the hits from.

Kits are drawn at random per mix: kicks from a soft 909 to a schranz kick clipped hard, with a
click as loud as their body or none; snares, claps and both; closed and open hats, rides and
shakers (all of them hats: what's high and short); toms, rims, congas, zaps and risers playing
unlabelled. Patterns follow the library's styles: techno and house on four, hard techno and
schranz, drum and bass, breaks; a bar or two can drop the kick or every drum. The mix ducks
under the kicks and is mastered loud as these genres are.
"""
import numpy as np
from scipy.signal import fftconvolve

import mixgen as G

SR = 44100
STYLES = ("techno", "hard", "house", "dnb", "breaks")


def kick(r, sr):
    style = r.choice(["909", "808", "techno", "hard", "dnb"])
    decay = {"909": (0.08, 0.3), "808": (0.3, 0.9), "techno": (0.1, 0.4), "hard": (0.15, 0.5), "dnb": (0.06, 0.25)}[style]
    decay = r.uniform(*decay)
    n = int(sr * min(5 * decay, 1.2))
    t = np.arange(n) / sr
    end = r.uniform(40, 65)
    f = end + end * r.uniform(1.5, 8) * np.exp(-t / r.uniform(0.004, 0.04))
    x = np.sin(2 * np.pi * np.cumsum(f) / sr) * G.env(n, sr, decay)
    if r.random() < 0.85:
        m = int(sr * r.uniform(0.001, 0.012))
        click = G.band(r.standard_normal(m), sr, lo=r.uniform(800, 4000)) if r.random() < 0.6 else np.sin(2 * np.pi * r.uniform(800, 4000) * np.arange(m) / sr)
        x[:m] += r.uniform(0.1, 1.2) * click / (np.abs(click).max() + 1e-9) * G.env(m, sr, r.uniform(0.001, 0.005))
    if style in ("techno", "hard") or r.random() < 0.3:
        g = r.uniform(2, 12) if style == "hard" else r.uniform(1, 5)
        x = np.tanh(g * x) / np.tanh(g)
        if style == "hard" and r.random() < 0.5:
            x = np.clip(x * r.uniform(1.2, 3), -1, 1)
    if r.random() < 0.3:
        # a top layer: a short noisy hit over the body, as layered kicks have
        m = int(sr * r.uniform(0.02, 0.06))
        top = G.band(r.standard_normal(m), sr, r.uniform(300, 1200), r.uniform(3000, 9000)) * G.env(m, sr, r.uniform(0.005, 0.02))
        x[:m] += r.uniform(0.1, 0.5) * top / (np.abs(top).max() + 1e-9)
    return x


def big_snare(r, sr):
    """A produced snare as drum and bass and breaks have them: a body of a few partials falling in
    pitch, the wires' noise up to the top of the spectrum with a crack at its start and a tail up
    to a third of a second, often a room and some drive."""
    n = int(sr * 0.6)
    t = np.arange(n) / sr
    f0 = r.uniform(150, 320)
    drop = 1 + r.uniform(0, 0.4) * np.exp(-t / r.uniform(0.005, 0.03))
    body = sum(a * np.sin(2 * np.pi * np.cumsum(f0 * k * drop) / sr) for k, a in ((1, 1.0), (1.6, r.uniform(0.2, 0.7)), (2.3, r.uniform(0, 0.4))))
    body = body * G.env(n, sr, r.uniform(0.03, 0.15))
    hi = r.uniform(9000, 18000) if r.random() < 0.6 else None
    wires = G.band(r.standard_normal(n), sr, lo=r.uniform(300, 2000), hi=hi) * G.env(n, sr, r.uniform(0.06, 0.3))
    m = int(sr * r.uniform(0.004, 0.015))
    wires[:m] *= r.uniform(1.5, 4)
    x = r.uniform(0.3, 1.0) * body / (np.abs(body).max() + 1e-9) + wires / (np.abs(wires).max() + 1e-9)
    if r.random() < 0.4:
        # a bright one, neurofunk's: the top louder than the body, as loud as a crash up there
        top = G.band(r.standard_normal(n), sr, lo=r.uniform(4000, 8000)) * G.env(n, sr, r.uniform(0.08, 0.35))
        x = r.uniform(0.1, 0.4) * x / (np.abs(x).max() + 1e-9) + top / (np.abs(top).max() + 1e-9)
    if r.random() < 0.6:
        tail = int(sr * r.uniform(0.1, 0.5))
        ir = r.standard_normal(tail) * G.env(tail, sr, r.uniform(0.03, 0.15))
        x = x + r.uniform(0.2, 0.6) * fftconvolve(x, ir / np.sqrt(np.sum(ir ** 2)))[:n]
    if r.random() < 0.5:
        g = r.uniform(1, 4)
        x = np.tanh(g * x / np.abs(x).max()) / np.tanh(g)
    return x


def wide_clap(r, sr):
    """A clap's bursts over a wider band, in a room: the house and techno kind."""
    x = G.clap(r, sr)
    noise = G.band(r.standard_normal(len(x)), sr, lo=r.uniform(400, 1200), hi=r.uniform(6000, 14000))
    x = x + r.uniform(0.3, 1.0) * noise * np.abs(x) / (np.abs(x).max() + 1e-9)
    if r.random() < 0.6:
        tail = int(sr * r.uniform(0.15, 0.6))
        ir = r.standard_normal(tail) * G.env(tail, sr, r.uniform(0.05, 0.2))
        x = x + r.uniform(0.2, 0.7) * fftconvolve(x, ir / np.sqrt(np.sum(ir ** 2)))[: len(x)]
    return x


def rim(r, sr):
    n = int(sr * 0.08)
    t = np.arange(n) / sr
    f = r.uniform(400, 1800)
    return (np.sin(2 * np.pi * f * t) + 0.5 * G.band(r.standard_normal(n), sr, lo=2000)) * G.env(n, sr, r.uniform(0.005, 0.02))


def zap(r, sr):
    n = int(sr * r.uniform(0.05, 0.3))
    t = np.arange(n) / sr
    f = r.uniform(200, 3000) * np.exp(-t / r.uniform(0.01, 0.08))
    return np.sign(np.sin(2 * np.pi * np.cumsum(f) / sr)) * G.env(n, sr, r.uniform(0.02, 0.1))


def ride(r, sr):
    n = int(sr * 0.8)
    t = np.arange(n) / sr
    base = r.uniform(300, 600)
    x = sum(np.sin(2 * np.pi * base * k * t + r.uniform(0, 6)) for k in (3.1, 4.7, 6.3, 8.9, 11.4, 13.7))
    x = x + 0.5 * r.standard_normal(n)
    return G.band(x, sr, lo=r.uniform(3000, 7000), order=4) * G.env(n, sr, r.uniform(0.2, 0.6))


def shaker(r, sr):
    n = int(sr * 0.06)
    x = r.standard_normal(n) * np.sin(np.pi * np.arange(n) / n) ** r.uniform(0.5, 3)
    return G.band(x, sr, lo=r.uniform(5000, 9000), order=4)


def riser(r, sr, n):
    t = np.arange(n) / sr
    x = r.standard_normal(n) * (t / t[-1]) ** r.uniform(1, 3)
    return G.band(x, sr, lo=r.uniform(500, 3000), hi=r.uniform(6000, 15000))


def kit(r, sr):
    k = {"kick": G.norm(kick(r, sr)), "hat": G.norm(G.hat(r, sr, False)), "open": G.norm(G.hat(r, sr, True)), "ride": G.norm(ride(r, sr)), "shaker": G.norm(shaker(r, sr)), "tom": G.norm(G.tom(r, sr)), "perc": G.norm(G.perc(r, sr)), "rim": G.norm(rim(r, sr)), "zap": G.norm(zap(r, sr))}
    kind = r.choice(["snare", "big", "clap", "wide", "both"], p=[0.15, 0.3, 0.15, 0.2, 0.2])
    s = {"snare": G.snare, "big": big_snare, "clap": G.clap, "wide": wide_clap, "both": big_snare}[kind](r, sr)
    if kind == "both":
        c = wide_clap(r, sr) if r.random() < 0.5 else G.clap(r, sr)
        s = np.pad(s, (0, max(0, len(c) - len(s))))
        s[: len(c)] += c
    k["snare"] = G.norm(s)
    return k


def pattern(r, style, bars):
    """(beat, voice, velocity) for every hit, beats counted from the start (16ths are 0.25)."""
    hits = []
    hat_kind = r.choice(["off", "eighths", "sixteenths"], p=[0.4, 0.3, 0.3])
    claps = r.random() < (0.85 if style in ("house", "dnb", "breaks") else 0.55)
    ride_on = r.random() < 0.3
    shaker_on = r.random() < 0.3
    perc_loop = [s for s in range(16) if r.random() < 0.12]
    for bar in range(bars):
        # a bar without kick (a breakdown's) or without drums, now and then
        no_kick = r.random() < 0.08
        if r.random() < 0.04:
            continue
        for s in range(16):
            b = bar * 4 + s / 4
            on = {}
            if style in ("techno", "hard", "house"):
                on["kick"] = s % 4 == 0
                on["snare"] = claps and s in (4, 12)
            elif style == "dnb":
                on["kick"] = s in (0, 10) or (s == 7 and r.random() < 0.3) or (s == 13 and r.random() < 0.2)
                on["snare"] = s in (4, 12) or (s in (7, 9, 14, 15) and r.random() < 0.12)
            else:
                on["kick"] = s in (0, 10) or (s in (3, 6, 7, 11, 14) and r.random() < 0.3)
                on["snare"] = s in (4, 12) or (s in (7, 9, 15) and r.random() < 0.15)
            if hat_kind == "off":
                on["open" if r.random() < 0.6 else "hat"] = s % 4 == 2
            elif hat_kind == "eighths":
                on["hat"] = s % 2 == 0 or (s % 4 == 2 and r.random() < 0.3)
            else:
                on["hat"] = r.random() < 0.9
            if ride_on and s % 4 == 2:
                on["ride"] = True
            if shaker_on and s % 2 == 1:
                on["shaker"] = r.random() < 0.8
            if s in perc_loop:
                on[r.choice(["perc", "rim", "tom"])] = True
            if r.random() < 0.02:
                on["zap"] = True
            if bar % 8 == 7 and s >= 12 and r.random() < 0.4:
                on["snare"] = True  # a fill
            if on.get("snare") and r.random() < 0.15:
                on["open"] = True  # an open hat on the snare: both heard
            for v, yes in on.items():
                if not yes or (no_kick and v == "kick"):
                    continue
                vel = r.uniform(0.85, 1.0) if v in ("kick", "snare") else r.uniform(0.4, 1.0)
                if v == "snare" and s not in (4, 12):
                    vel *= r.uniform(0.3, 0.8)  # a ghost note: a snare still
                hits.append((b, v, vel))
    return hits


LABEL = {"kick": 0, "snare": 1, "hat": 2, "open": 2, "ride": 2, "shaker": 2}


# how often each style is drawn; DNB_HEAVY in the environment leans on drum and bass and breaks
STYLE_P = [0.15, 0.15, 0.05, 0.45, 0.2] if __import__("os").environ.get("DNB_HEAVY") else [0.3, 0.25, 0.1, 0.25, 0.1]


def drums(r, sr, n, style=None):
    """Mono drums for `n` samples, the kicks' envelope (to duck under), and each hit's exact time
    per label (kick, snare, hat)."""
    style = style or r.choice(STYLES, p=STYLE_P)
    bpm = r.uniform(*{"techno": (126, 148), "hard": (145, 170), "house": (118, 128), "dnb": (165, 178), "breaks": (128, 142)}[style])
    beat = 60 / bpm
    hits = pattern(r, style, int(n / sr / beat / 4) + 2)
    k = kit(r, sr)
    loud_snare = style in ("dnb", "breaks")
    gains = {"kick": r.uniform(-3, 0), "snare": r.uniform(-4, 3) if loud_snare else r.uniform(-10, 0), "hat": r.uniform(-24, -8), "open": r.uniform(-24, -8), "ride": r.uniform(-26, -12), "shaker": r.uniform(-28, -14), "tom": r.uniform(-14, -4), "perc": r.uniform(-20, -8), "rim": r.uniform(-20, -8), "zap": r.uniform(-20, -8)}
    swing = r.uniform(0, 0.06) if r.random() < 0.3 else 0.0
    out, kicks = np.zeros(n + 2 * sr), np.zeros(n + 2 * sr)
    labels = [[], [], []]
    for b, v, vel in hits:
        sixteenth = round((b % 1) * 4)
        t = (b + (swing if sixteenth % 2 == 1 else 0.0)) * beat + r.normal(0, 0.001)
        i = int(max(0.0, t) * sr)
        if i >= n:
            continue
        sound = k[v] * vel * 10 ** (gains[v] / 20)
        out[i:i + len(sound)] += sound
        if v == "kick":
            m = int(0.25 * sr)
            kicks[i:i + m] = np.maximum(kicks[i:i + m], np.linspace(1, 0, m))
        if v in LABEL:
            labels[LABEL[v]].append(i / sr)
    if r.random() < 0.3:
        m = int(sr * r.uniform(2, 8))
        at = int(r.integers(0, max(1, n - m)))
        out[at:at + m] += riser(r, sr, m)[: len(out) - at] * 10 ** (r.uniform(-24, -10) / 20)
    if r.random() < 0.4:
        tail = int(sr * r.uniform(0.2, 1.0))
        ir = r.standard_normal(tail) * G.env(tail, sr, r.uniform(0.05, 0.3))
        out = out + r.uniform(0.05, 0.25) * fftconvolve(out, ir / np.sqrt(np.sum(ir ** 2)))[: len(out)]
    return out[:n], kicks[:n], [sorted(set(l)) for l in labels]


def mix(r, rest, sr):
    """Generated drums over `rest` (a real minute's bass and melody): the mix, the drums alone,
    and the hits. The rest ducks under the kicks, the whole is mastered loud."""
    n = len(rest)
    d, kicks, labels = drums(r, sr, n)
    rms = lambda a: np.sqrt(np.mean(a ** 2)) + 1e-9
    if r.random() < 0.6:
        rest = rest * (1 - r.uniform(0.3, 0.9) * kicks)
    d = d * rms(rest) / rms(d) * 10 ** (r.uniform(-4, 8) / 20)
    if r.random() < 0.08:
        rest = rest * 0  # drums alone
    x = d + rest
    peak = np.abs(x).max() + 1e-9
    if r.random() < 0.7:
        # loud masters: driven into a soft clip
        g = r.uniform(1.5, 5)
        x = np.tanh(g * x / peak) / np.tanh(g)
        d = np.tanh(g * d / peak) / np.tanh(g)
    else:
        x, d = x / peak, d / peak
    level = 10 ** (r.uniform(-8, -0.5) / 20)
    return (x * level).astype(np.float32), (d * level).astype(np.float32), labels
