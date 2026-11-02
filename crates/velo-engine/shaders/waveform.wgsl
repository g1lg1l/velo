/*{
  "name": "Waveform",
  "category": "generator",
  "description": "The music drawn: its spectrum as bars mirrored round the middle or round a ring that breathes with the bass, a glowing scope that holds still on a steady tone, or rings the beat sends out; low to high, from one color to the other",
  "audio": true,
  "params": [
    { "id": "mode", "type": "int", "default": 0, "names": ["Spectrum", "Radial", "Scope", "Rings"] },
    { "id": "color_a", "type": "color", "default": [0.13, 0.83, 0.93, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.75, 0.35, 1.0, 1.0] },
    { "id": "gain", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "bars", "type": "int", "default": 48, "min": 4, "max": 128 },
    { "id": "gap", "type": "float", "default": 0.35, "min": 0.0, "max": 0.9 },
    { "id": "thickness", "type": "float", "default": 0.006, "min": 0.001, "max": 0.05 },
    { "id": "glow", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "radius", "type": "float", "default": 0.22, "min": 0.05, "max": 0.45 },
    { "id": "mirror", "type": "bool", "default": true },
    { "id": "past", "type": "float", "default": 1.0, "min": 0.1, "max": 1.0 }
  ]
}*/

// wave's rows: 0 the last 512 hops' levels (level, bass, mid, high), oldest first; 1 the last 512
// samples, from where the wave rose through zero; 2 the spectrum, 30 Hz to 16 kHz evenly in
// octaves, 0..1, already falling slowly. color_a is the low end, color_b the high one. bars: the
// spectrum's bars (Spectrum, Radial) or the rings out to the edge (Rings); gap: the room between
// them; radius: the ring Radial stands on; mirror: Spectrum up and down from the middle, Radial
// the same both sides, Scope drawn upside down too; past: how much of it Scope and Rings show.
// The picture's alpha is what's drawn: over a background only the music shows.

const TAU: f32 = 6.2831853;

// the spectrum's loudest between x0 and x1 (0 low, 1 high), turned up by gain
fn band(x0: f32, x1: f32) -> f32 {
    let a = clamp(x0, 0.0, 1.0) * 511.0;
    let b = clamp(x1, 0.0, 1.0) * 511.0;
    var m = 0.0;
    for (var k = 0; k <= 8; k++) {
        m = max(m, textureLoad(wave, vec2i(i32(mix(a, b, f32(k) / 8.0)), 2), 0).r);
    }
    return clamp(m * gain, 0.0, 1.0);
}

// the levels t of the way from the oldest hop shown to the newest, interpolated by hand: the
// audio texture is unfiltered
fn history(t: f32) -> vec4f {
    let x = 511.0 - (1.0 - clamp(t, 0.0, 1.0)) * 511.0 * past;
    let i = i32(floor(x));
    return mix(textureLoad(wave, vec2i(i, 0), 0), textureLoad(wave, vec2i(min(i + 1, 511), 0), 0), fract(x));
}

// a lit shape at signed distance d (canvas heights): its body and its glow, premultiplied
fn lit(color: vec3f, d: f32, strength: f32) -> vec4f {
    let aa = 1.0 / g.resolution.y;
    let body = 1.0 - smoothstep(-aa, aa, d);
    let halo = glow * 0.6 * exp(-max(d, 0.0) / (0.004 + glow * 0.03));
    let a = clamp(max(body, halo) * strength, 0.0, 1.0);
    return vec4f(color * a, a);
}

fn over(a: vec4f, b: vec4f) -> vec4f {
    return a + b * (1.0 - a.a);
}

// the color x of the way from the low end to the high one, brighter as it's louder
fn tone(x: f32, loud: f32) -> vec3f {
    return mix(color_a.rgb, color_b.rgb, x) * (0.75 + 0.5 * loud);
}

// bars of the spectrum across the picture, rounded, up and down from the middle when mirrored
fn spectrum(uv: vec2f) -> vec4f {
    let n = f32(bars);
    let i = floor(uv.x * n);
    let v = band(i / n, (i + 1.0) / n);
    let w = (1.0 - gap) * 0.5 / n * g.aspect;
    let x = (fract(uv.x * n) - 0.5) / n * g.aspect;
    var d: f32;
    if mirror {
        let h = v * 0.42;
        d = length(vec2f(x, max(abs(uv.y - 0.5) - h, 0.0))) - w;
    } else {
        let h = v * 0.85;
        let y = 0.95 - uv.y;
        d = length(vec2f(x, max(abs(y - h * 0.5) - h * 0.5, 0.0))) - w;
    }
    return lit(tone(i / n, v), d, 1.0);
}

// the spectrum round a ring that breathes with the bass, lows at the top
fn radial(uv: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let r = length(p);
    let base = radius * (1.0 + 0.12 * g.bass);
    // turns from twelve o'clock, clockwise; mirrored, the same both sides
    let turn = fract(atan2(p.x, -p.y) / TAU + 1.0);
    let x = select(turn, 1.0 - abs(turn * 2.0 - 1.0), mirror);
    let n = f32(bars);
    let i = floor(x * n);
    let v = band(i / n, (i + 1.0) / n);
    // a bar's width along the ring, where it stands
    let arc = select(TAU, TAU * 0.5, mirror) * base / n;
    let u = (fract(x * n) - 0.5) * arc;
    let h = v * 0.22;
    let out = r - base - thickness * 2.0 - h * 0.5;
    let bar = length(vec2f(u, max(abs(out) - h * 0.5, 0.0))) - (1.0 - gap) * 0.5 * arc;
    var o = lit(tone(i / n, v), bar, 1.0);
    // the ring itself, and its inside lit by the kick
    o = over(o, lit(color_a.rgb, abs(r - base) - thickness * 0.5, 1.0));
    let inside = (1.0 - smoothstep(base - 0.01, base, r)) * 0.25 * g.kick;
    return over(o, vec4f(color_a.rgb * inside, inside));
}

// the scope: the last samples as a glowing line, thicker as it's louder
fn scope(uv: vec2f) -> vec4f {
    let span = 511.0 * past;
    let first = 511.0 - span;
    let at = i32(floor(first + uv.x * span));
    let p = vec2f(uv.x * g.aspect, uv.y);
    let lift = 0.42 * gain;
    var d = 100.0;
    var flipped = 100.0;
    var y = 0.0;
    // the segments near x, as far as the glow reaches
    let reach = i32(ceil(0.05 * span / g.aspect)) + 1;
    for (var j = -reach; j <= reach; j++) {
        let i = clamp(at + j, 0, 510);
        let a = textureLoad(wave, vec2i(i, 1), 0).r;
        let b = textureLoad(wave, vec2i(i + 1, 1), 0).r;
        let x0 = (f32(i) - first) / span * g.aspect;
        let x1 = (f32(i + 1) - first) / span * g.aspect;
        for (var side = 0; side < 2; side++) {
            let s = select(-1.0, 1.0, side == 1);
            let start = vec2f(x0, 0.5 + s * a * lift);
            let edge = vec2f(x1, 0.5 + s * b * lift) - start;
            let k = clamp(dot(p - start, edge) / max(dot(edge, edge), 1e-9), 0.0, 1.0);
            let e = length(p - start - edge * k);
            if side == 0 {
                d = min(d, e);
            } else {
                flipped = min(flipped, e);
            }
        }
        if j == 0 {
            y = a;
        }
    }
    let half = thickness * (0.5 + 1.5 * g.level);
    var o = lit(tone(uv.x, abs(y)), d - half, 1.0);
    if mirror {
        o = over(o, lit(tone(1.0 - uv.x, abs(y)), flipped - half, 0.5));
    }
    // a faint fill from the line to the middle
    let top = 0.5 - y * lift;
    let fill = 0.12 * step(min(0.5, top), uv.y) * step(uv.y, max(0.5, top));
    return over(o, vec4f(tone(uv.x, 0.0) * fill, fill));
}

// rings the music sends out from the middle: the newest at the center, a ring's light the bass
// when it left, its color the highs
fn rings_out(uv: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let edge = 0.5 * length(vec2f(g.aspect, 1.0));
    let k = length(p) / edge;
    let n = f32(bars) * 0.25;
    // which ring, and how far from its line
    let ring = floor(k * n);
    let w = abs(fract(k * n) - 0.5) / n * edge;
    let h = history(1.0 - (ring + 0.5) / n);
    // the peaks stand out, a steady bass stays dim
    let v = pow(clamp(h.g * gain, 0.0, 1.0), 3.0);
    let line = w - (1.0 - gap) * 0.5 / n * edge * v;
    return lit(tone(clamp(h.a * 2.0, 0.0, 1.0), v), line, v * (1.0 - k * 0.8));
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    if mode == 1 {
        return radial(uv);
    }
    if mode == 2 {
        return scope(uv);
    }
    if mode == 3 {
        return rings_out(uv);
    }
    return spectrum(uv);
}
