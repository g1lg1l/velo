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
    { "id": "rotate", "type": "float", "default": 0.0, "min": -1.0, "max": 1.0 },
    { "id": "_now", "type": "float", "default": 0.0, "min": 0.0, "max": 255.0 },
    { "id": "_before", "type": "float", "default": -1.0, "min": -1.0, "max": 255.0 },
    { "id": "_turn", "type": "float", "default": 1.0, "min": 0.0, "max": 1.0 }
  ]
}*/

// Its words are the list's items, a word flagged in its record's last float starting each (an
// item may run over lines); the drawing is text-draw.wgsl's. type_in: the share of each turn spent
// typing the item in. Which item shows is Velo's to say, as a Media List's file is: the line
// (_now), the one before it, fading out (_before, -1 on the first turn), and how far through its
// turn it is (_turn), from order, clock and every.

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
    // Velo counts the turns and picks each one's line (media.rs, Lines): at random is a deck
    // shuffled anew every round, which can't be dealt here, where every pixel works alone
    let k = _turn;
    let typed = select(1.0, clamp(k / type_in, 0.0, 1.0), type_in > 0.0);
    let now = show_line(clamp(i32(round(_now)), 0, n - 1), all, typed, uv);
    // the first part of each turn crossfades from the one before (none on the first turn)
    if fade > 0.0 && k < fade && _before >= 0.0 {
        return mix(show_line(min(i32(round(_before)), n - 1), all, 1.0, uv), now, smoothstep(0.0, 1.0, k / fade));
    }
    return now;
}
