/*{
  "name": "Shapes",
  "category": "generator",
  "description": "A shape in rings: polygon, star, heart and more, outlined or filled, with a glow, stretched, twisted ring by ring, tiled across the picture. Angle turns them (in turns: Time onto it at 100 % is a turn a period), spin keeps them turning, expand sends the rings outward, wobble waves their outline, dashes cut it and march it round, split pulls red and blue apart, pulse makes them jump on the kick",
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Polygon", "Circle", "Ring", "Star", "Heart", "Cross", "Flower", "Rounded rect", "Crescent", "Arc", "Teardrop", "Vesica", "Triangle", "Diamond", "Line"] },
    { "id": "sides", "type": "int", "default": 6, "min": 3, "max": 12 },
    { "id": "size", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "thickness", "type": "float", "default": 0.012, "min": 0.002, "max": 0.2 },
    { "id": "spin", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "angle", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "rings", "type": "int", "default": 4, "min": 1, "max": 12 },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "tint_out", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "fill", "type": "bool", "default": false },
    { "id": "glow", "type": "float", "default": 0.35, "min": 0.0, "max": 1.0 },
    { "id": "spacing", "type": "float", "default": 0.45, "min": 0.1, "max": 2.0 },
    { "id": "pulse", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "inner", "type": "float", "default": 0.5, "min": 0.1, "max": 0.95 },
    { "id": "roundness", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "stretch", "type": "vec2", "default": [1.0, 1.0], "min": 0.1, "max": 4.0 },
    { "id": "twist", "type": "float", "default": 0.0, "min": -0.5, "max": 0.5 },
    { "id": "softness", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "fade", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "hue_step", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "expand", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "wobble", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "waves", "type": "int", "default": 5, "min": 1, "max": 24 },
    { "id": "dash", "type": "float", "default": 0.0, "min": 0.0, "max": 0.95 },
    { "id": "dashes", "type": "int", "default": 12, "min": 1, "max": 64 },
    { "id": "march", "type": "float", "default": 0.0, "min": -2.0, "max": 2.0 },
    { "id": "split", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "tile", "type": "int", "default": 1, "min": 1, "max": 8 },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.0, 0.0] }
  ]
}*/

// Every shape is a signed distance at size 1 (negative inside), scaled to each ring's size.
// sides: a polygon's sides, a star's points, a flower's petals. inner: a ring's hole, a star's
// inner radius, how deep a crescent's bite is, how much of the circle an arc covers. Each ring
// out turns by twist (in turns) and its color by hue_step; fade dims the outer ones; tile
// repeats the whole across the picture in tile × tile cells. The rings' color runs from tint,
// the innermost, to tint_out. expand: rings a second flowing out (in, below 0), each born at the
// middle and gone past the last. wobble: the outline waved `waves` times round; dash: the share
// of the outline cut out of each of its dashes, march turning them round (turns a second); split:
// red and blue drawn apart. audio: the bass breathes, the kick flares the glow, the level thickens.

fn box(p: vec2f, half: vec2f) -> f32 {
    let d = abs(p) - half;
    return length(max(d, vec2f(0.0))) + min(max(d.x, d.y), 0.0);
}

// distance to the segment a b, negative on the side the center is
fn edge(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let d = length(pa - ba * clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0));
    return select(d, -d, ba.x * pa.y - ba.y * pa.x > 0.0);
}

// p folded into the half wedge from angle 0 to half a sector, by its angle from `start`
fn fold(p: vec2f, n: f32, start: f32) -> vec2f {
    let sector = 6.2831853 / n;
    let a = atan2(p.y, p.x) - start;
    let b = abs(a - floor(0.5 + a / sector) * sector);
    return length(p) * vec2f(cos(b), sin(b));
}

// a regular polygon whose sides touch the unit circle, a side facing angle `start`
fn polygon(p: vec2f, n: f32, start: f32) -> f32 {
    let s = fold(p, n, start);
    // past the corners, the true distance rounds the outline; the plain one keeps them sharp
    let corner = vec2f(1.0, clamp(s.y, 0.0, tan(3.1415927 / n)));
    return select(s.x - 1.0, length(s - corner) * sign(s.x - 1.0), roundness > 0.0);
}

fn base(p: vec2f) -> f32 {
    let n = f32(sides);
    let q = vec2f(abs(p.x), p.y);
    if shape == 1 {
        return length(p) - 1.0;
    }
    if shape == 2 {
        let w = (1.0 - inner) * 0.5;
        return abs(length(p) - 1.0 + w) - w;
    }
    if shape == 3 {
        let s = fold(p, n, 1.5707963);
        let a = 3.1415927 / n;
        return edge(s, vec2f(1.0, 0.0), inner * vec2f(cos(a), sin(a)));
    }
    if shape == 4 {
        // a square on its corner and two circles on its upper sides
        let h = 0.85;
        let c = -0.1035 * h;
        let u = vec2f(q.x + q.y - c, q.y - c - q.x) * 0.70710678;
        let lobes = length(q - vec2f(h * 0.5, c + h * 0.5)) - h * 0.70710678;
        return min(box(u, vec2f(h * 0.70710678)), lobes);
    }
    if shape == 5 {
        return min(box(p, vec2f(1.0, 0.3)), box(p, vec2f(0.3, 1.0)));
    }
    if shape == 6 {
        // the radius swings with the angle; dividing by the slope keeps the outline even
        let r = length(p);
        let a = atan2(p.y, p.x) * n * 0.5;
        let petal = inner + (1.0 - inner) * abs(cos(a));
        let slope = (1.0 - inner) * n * 0.5 * sin(a) / max(r, 0.001);
        return (r - petal) / sqrt(1.0 + slope * slope);
    }
    if shape == 7 {
        return box(p, vec2f(0.8, 0.5)) - 0.2;
    }
    if shape == 8 {
        // a circle with a bite out of it: the nearest of its outer arc, the bite's arc and the horns
        let o = vec2f(1.2 * (1.0 - inner), 0.0);
        let s = vec2f(p.x, abs(p.y));
        let b = s - o;
        let horn = vec2f(o.x * 0.5, sqrt(1.0 - o.x * o.x * 0.25));
        let tip = length(s - horn);
        let outer = select(tip, abs(length(s) - 1.0), horn.x * s.y >= horn.y * s.x);
        let h = horn - o;
        let bitten = select(tip, abs(length(b) - 1.0), h.x * b.y >= h.y * b.x);
        let d = min(outer, bitten);
        return select(d, -d, length(s) < 1.0 && length(b) > 1.0);
    }
    if shape == 9 {
        // a part of the unit circle, centered on the top, with round ends
        let half = inner * 3.1415927;
        let a = atan2(q.x, q.y);
        let end = vec2f(sin(half), cos(half));
        return select(length(q - end), abs(length(p) - 1.0), a < half);
    }
    if shape == 10 {
        // a circle and a point above it, joined by their tangents
        let r = 0.6;
        let h = 1.4;
        let t = q - vec2f(0.0, -0.4);
        let b = r / h;
        let a = sqrt(1.0 - b * b);
        let k = dot(t, vec2f(-b, a));
        if k < 0.0 {
            return length(t) - r;
        }
        if k > a * h {
            return length(t - vec2f(0.0, h));
        }
        return dot(t, vec2f(a, b)) - r;
    }
    if shape == 11 {
        // where two circles through each other's centers overlap; past a tip, the tip is nearest
        let r = 1.1547005;
        let s = abs(p);
        let c = s + vec2f(r * 0.5, 0.0);
        return select(length(c) - r, length(s - vec2f(0.0, 1.0)), r * 0.5 * s.y > c.x);
    }
    if shape == 12 {
        return polygon(p * 2.0, 3.0, -1.5707963) * 0.5;
    }
    if shape == 13 {
        let s = abs(p);
        return edge(s, vec2f(0.7, 0.0), vec2f(0.0, 1.0));
    }
    if shape == 14 {
        return length(vec2f(p.x - clamp(p.x, -1.0, 1.0), p.y));
    }
    return polygon(p, n, 0.0);
}

// the shape shrunk and then grown back by the same amount: the corners come out round
fn unit(p: vec2f) -> f32 {
    let r = roundness * 0.9;
    let k = 1.0 - r;
    return base(p / k) * k - r;
}

// a color turned round the grey axis
fn hue_turn(c: vec3f, turns: f32) -> vec3f {
    let k = vec3f(0.57735027);
    let a = turns * 6.2831853;
    return c * cos(a) + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - cos(a));
}

// the rings at p: their color (premultiplied by coverage) and coverage
fn rings_at(p: vec2f) -> vec4f {
    let thick = thickness * (1.0 + audio * g.level);
    // the edge: sharp at softness 0, a soft falloff up to five times the line at 1
    let e0 = thick * 0.5 * (1.0 - softness);
    let e1 = thick * (1.0 + 4.0 * softness);
    let shine = glow + 0.5 * audio * g.kick;
    // expanding: each ring a step further out each 1/expand seconds
    let flow = select(0.0, fract(g.time * expand), expand != 0.0);
    let last = max(f32(rings - 1), 1.0);
    var v = 0.0;
    var c = vec3f(0.0);
    for (var i = 0; i < rings; i++) {
        let k = f32(i) + flow;
        // angle in turns, spin in radians a second, twist in turns a ring
        let a = (angle + twist * k) * 6.2831853 + g.time * spin;
        // turned, then flipped so that y points up
        let q = vec2f(p.x * cos(a) - p.y * sin(a), -p.x * sin(a) - p.y * cos(a));
        let r = max(size * (1.0 + k * spacing) * (1.0 + pulse * g.kick + 0.25 * audio * g.bass), 0.0001);
        let around = atan2(q.y, q.x);
        var d = unit(q / r);
        if wobble > 0.0 {
            d += wobble * 0.15 * sin(around * f32(waves) + g.time * 0.5 + k);
        }
        d *= r;
        let e = abs(d);
        var ring = 1.0 - smoothstep(e0, e1, e);
        if dash > 0.0 {
            let u = fract(around / 6.2831853 * f32(dashes) - g.time * march);
            ring *= smoothstep(dash - 0.03, dash + 0.03, u);
        }
        // the innermost one filled
        if fill && i == 0 {
            ring = max(ring, 1.0 - smoothstep(-thick * (0.5 + 4.5 * softness), 0.0, d));
        }
        if shine > 0.0 {
            ring = max(ring, shine * exp(-e / (0.01 + shine * 0.12)));
        }
        ring *= 1.0 - fade * k / last;
        // flowing: a ring fades in at the middle and out past the last
        if expand != 0.0 {
            ring *= clamp(k, 0.0, 1.0) * clamp(f32(rings) - k, 0.0, 1.0);
        }
        v = max(v, ring);
        let tone = mix(tint.rgb, tint_out.rgb, clamp(k / last, 0.0, 1.0));
        c = max(c, hue_turn(tone, hue_step * k) * ring);
    }
    return vec4f(c, v);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // tiled: each cell a small copy of the picture
    let n = f32(max(tile, 1));
    let cell = select(uv - 0.5 - position * 0.5, fract((uv - position * 0.5) * n) - 0.5, tile > 1);
    let p = cell * vec2f(g.aspect, 1.0) / max(stretch, vec2f(0.01));
    var o = rings_at(p);
    // split: red one way and blue the other, green where it was
    if split > 0.0 {
        let off = vec2f(split * 0.03, 0.0);
        let red = rings_at(p - off);
        let blue = rings_at(p + off);
        o = vec4f(red.r, o.g, blue.b, max(max(red.a, o.a), blue.a));
    }
    // alpha is the coverage: over another picture (or the background) only the rings show
    let shape = vec4f(o.rgb * tint.a, o.a * tint.a);
    return shape + vec4f(background.rgb * background.a, background.a) * (1.0 - shape.a);
}
