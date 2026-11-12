/*{
  "name": "Text List",
  "category": "generator",
  "description": "Lines of words shown one at a time: in order, at random (every line once before any comes again, another way round each time) or back and forth, a new one every so many beats or seconds, cut or crossfaded, typed in if you like; set in a font as the Text sets its words",
  "inputs": [{ "id": "src", "type": "texture" }],
  "params": [
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 },
    { "id": "order", "type": "int", "default": 0, "names": ["In order", "At random", "Back and forth"] },
    { "id": "clock", "type": "int", "default": 0, "names": ["Beats", "Seconds"] },
    { "id": "every", "type": "float", "default": 4.0, "min": 0.25, "max": 64.0 },
    { "id": "fade", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "type_in", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "font", "type": "int", "default": 0, "names": ["Inter", "Mono", "File"] },
    { "id": "size", "type": "float", "default": 0.15, "min": 0.01, "max": 1.0 },
    { "id": "align", "type": "int", "default": 1, "names": ["Left", "Center", "Right"] },
    { "id": "tracking", "type": "float", "default": 0.0, "min": -0.1, "max": 1.0 },
    { "id": "line_height", "type": "float", "default": 1.0, "min": 0.5, "max": 3.0 },
    { "id": "wrap", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "weight", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "slant", "type": "float", "default": 0.0, "min": -0.5, "max": 0.5 },
    { "id": "uppercase", "type": "bool", "default": false },
    { "id": "tint", "type": "color", "default": [1.0, 1.0, 1.0, 1.0] },
    { "id": "opacity", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "outline", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "outline_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "shadow", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "shadow_offset", "type": "vec2", "default": [0.04, 0.06], "min": -0.5, "max": 0.5 },
    { "id": "shadow_blur", "type": "float", "default": 0.3, "min": 0.0, "max": 1.0 },
    { "id": "shadow_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "box", "type": "float", "default": 0.0, "min": 0.0, "max": 1.0 },
    { "id": "box_color", "type": "color", "default": [0.0, 0.0, 0.0, 1.0] },
    { "id": "box_padding", "type": "float", "default": 0.2, "min": 0.0, "max": 1.0 },
    { "id": "box_radius", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "reveal", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 },
    { "id": "position", "type": "vec2", "default": [0.0, 0.0], "min": -1.0, "max": 1.0 },
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 }
  ]
}*/

// Its words are the list's items, a word flagged in its record's last float starting each (an
// item may run over lines); the drawing is text-draw.wgsl's. type_in: the share of each turn spent
// typing the item in.

// how many items the list has
fn entries() -> i32 {
    var n = 0;
    for (var i = 0; i < nwords; i++) {
        if i == 0 || num(words + i * 8 + 7) > 0.5 {
            n += 1;
        }
    }
    return n;
}

// item k's words: its first (from all's first record) and how many
fn entry(k: i32, all: vec2i) -> vec2i {
    var n = -1;
    var first = 0;
    for (var i = 0; i < all.y; i++) {
        if i == 0 || num(all.x + i * 8 + 7) > 0.5 {
            n += 1;
            if n == k {
                first = i;
            } else if n == k + 1 {
                return vec2i(first, i - first);
            }
        }
    }
    return vec2i(first, all.y - first);
}

fn hash(x: f32) -> f32 {
    return fract(sin(x * 12.9898 + 78.233) * 43758.5453);
}

fn gcd(a: i32, b: i32) -> i32 {
    var x = a;
    var y = b;
    for (var i = 0; i < 16 && y != 0; i++) {
        let t = x % y;
        x = y;
        y = t;
    }
    return x;
}

// the line dealt `i`th in round `round` of n lines: a stride through them that shares no
// factor with n (so every line comes once) from a start of its own, both new every round.
// Worked out for every pixel, so no deck can be shuffled here: a stride costs nothing
fn dealt(i: i32, round: i32, n: i32) -> i32 {
    var stride = 1 + i32(hash(f32(round) * 1.7 + 3.0) * f32(n - 1)) % (n - 1);
    for (var k = 0; k < 16 && gcd(stride, n) != 1; k++) {
        stride = stride % (n - 1) + 1;
    }
    stride = select(1, stride, gcd(stride, n) == 1);
    let start = i32(hash(f32(round) * 2.3 + 11.0) * f32(n)) % n;
    return (stride * i + start) % n;
}

// the line a step of the clock shows (as the Media List picks its files)
fn pick(step: i32, n: i32) -> i32 {
    if n <= 1 {
        return 0;
    }
    if order == 1 {
        // at random as a deck is dealt: every line once, then all of them again another way
        // round (as the Media List deals its files). A round doesn't start on the line the
        // one before ended on: its first two swap
        let round = step / n;
        let i = step % n;
        let clash = round > 0 && dealt(0, round, n) == dealt(n - 1, round - 1, n);
        return dealt(select(i, 1 - i, clash && i < 2 && n > 2), round, n);
    }
    if order == 2 {
        let period = 2 * n - 2;
        let s = step % period;
        return select(s, period - s, s >= n);
    }
    return step % n;
}

// line k drawn at uv, `typed` of its characters in
fn show_line(k: i32, all: vec2i, typed: f32, uv: vec2f) -> vec4f {
    let r = entry(k, all);
    words = all.x + r.x * 8;
    nwords = r.y;
    if nwords == 0 {
        return vec4f(0.0);
    }
    // its characters, by their place in the whole text
    let a = quad(words);
    let z = quad(words + (nwords - 1) * 8);
    let first = quad(i32(a.x) + 4).w;
    let last = quad(i32(z.x) + max(i32(z.y) - 1, 0) * 8 + 4).w + 1.0;
    shown = first + reveal * typed * (last - first);
    return draw(uv);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    setup();
    let all = vec2i(words, nwords);
    let n = entries();
    if n == 0 {
        return vec4f(0.0);
    }
    let t = select(g.beats, g.time, clock == 1) / max(every, 0.01);
    // a clock moved back (a resync, a tap) doesn't count below the first
    let step = max(i32(floor(t)), 0);
    let k = fract(t);
    let typed = select(1.0, clamp(k / type_in, 0.0, 1.0), type_in > 0.0);
    let now = show_line(pick(step, n), all, typed, uv);
    // the first part of each turn crossfades from the one before
    if fade > 0.0 && k < fade && step > 0 {
        return mix(show_line(pick(step - 1, n), all, 1.0, uv), now, smoothstep(0.0, 1.0, k / fade));
    }
    return now;
}
