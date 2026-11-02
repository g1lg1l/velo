// src is the glyphs' fields and their layout (velo_engine::text; the engine binds it, it's not a
// port). Lengths are in units: a line's height at size 1, so a line is `size` of the canvas'
// height and a font's ascent to descent 0.8 of it. Tracking, shadow offset, padding and radius
// are in units too; wrap is a share of the canvas' width.

// how far a glyph's cell reaches from its line's middle or past its word's ends
const REACH: f32 = 0.8;
// how far weight, outline and shadow blur at 1 move the edge out: together the field's reach
const BOLD: f32 = 0.04;
const LINE: f32 = 0.08;
const BLUR: f32 = 0.16;

var<private> cols: i32;
// atlas texels a unit takes, how far a field reaches past an edge, the atlas' first row
var<private> unit: f32;
var<private> reach: f32;
var<private> top: f32;
// the font's words (their first record, how many), its ascent, the characters showing
var<private> words: i32;
var<private> nwords: i32;
var<private> ascent: f32;
var<private> shown: f32;
// where lines break (0: only at newlines), how many there are, the widest
var<private> limit: f32;
// the size drawn at: size, pumped by the kick
var<private> sz: f32;
var<private> lines: i32;
var<private> widest: f32;

// a float of the layout: one in each texel's four bytes, from the top left
fn num(i: i32) -> f32 {
    return bitcast<f32>(pack4x8unorm(textureLoad(src, vec2i(i % cols, i / cols), 0)));
}

fn quad(i: i32) -> vec4f {
    return vec4f(num(i), num(i + 1), num(i + 2), num(i + 3));
}

// a line's width with x of advances in it: its last letter's tracking left out
fn span(x: f32) -> f32 {
    return select(0.0, x - tracking, x > 0.0);
}

// whether word i (adv wide, after a gap) starts a line that is x wide so far
fn starts_line(i: i32, x: f32, gap: f32, adv: f32, newline: f32) -> bool {
    return i > 0 && (newline > 0.5 || (limit > 0.0 && x > 0.0 && x + gap + adv - tracking > limit));
}

// how many lines the words break into, and the widest
fn measure() {
    var x = 0.0;
    var gap = 0.0;
    for (var i = 0; i < nwords; i++) {
        let a = quad(words + i * 8);
        let b = quad(words + i * 8 + 4);
        let adv = a.z + a.w * tracking;
        if starts_line(i, x, gap, adv, b.z) {
            widest = max(widest, span(x));
            lines += 1;
            x = adv;
        } else {
            x += gap + adv;
        }
        gap = b.x + b.y * tracking;
    }
    if nwords > 0 {
        widest = max(widest, span(x));
        lines += 1;
    }
}

// how much the letters cover p (units from the block's middle) with their edges moved out by
// edges.x, and by edges.y, each spread over soft; the letter reveal is on fades in
fn cover(p: vec2f, edges: vec2f, soft: f32) -> vec2f {
    let half = vec2f(widest, f32(lines) * line_height) * 0.5;
    if lines == 0 || abs(p.x) > half.x + REACH + abs(slant) || abs(p.y) > half.y + REACH {
        return vec2f(0.0);
    }
    // the lines whose glyphs reach p: their middles within REACH of it
    let l0 = max(i32(ceil((p.y + half.y - REACH) / line_height - 0.5)), 0);
    let l1 = min(i32(floor((p.y + half.y + REACH) / line_height - 0.5)), l0 + 3);
    var acc = vec2f(0.0);
    var line = 0;
    var start = 0;
    var x = 0.0;
    var gap = 0.0;
    // the lines break as measure() broke them; each one near p is drawn once it's closed
    for (var i = 0; i <= nwords; i++) {
        var adv = 0.0;
        var after = 0.0;
        var ends = i > 0;
        if i < nwords {
            let a = quad(words + i * 8);
            let b = quad(words + i * 8 + 4);
            adv = a.z + a.w * tracking;
            after = b.x + b.y * tracking;
            ends = starts_line(i, x, gap, adv, b.z);
        }
        if ends {
            if line >= l0 && line <= l1 {
                acc = max(acc, in_line(p, line, start, i, span(x), edges, soft));
            }
            line += 1;
            if line > l1 {
                break;
            }
            start = i;
            x = adv;
        } else {
            x += gap + adv;
        }
        gap = after;
    }
    return acc;
}

// cover, for line number `line`: words start to end, lw wide
fn in_line(p: vec2f, line: i32, start: i32, end: i32, lw: f32, edges: vec2f, soft: f32) -> vec2f {
    // the glyphs' height in the middle of the line's; the line where align puts it in the block
    let baseline = (f32(line) - 0.5 * f32(lines)) * line_height + 0.5 * (line_height - 0.8) + ascent;
    let left = (widest - lw) * 0.5 * f32(align) - 0.5 * widest;
    // a glyph's point h above the baseline is drawn slant * h further right
    let x = p.x - slant * (baseline - p.y) - left;
    let y = p.y - baseline;
    var acc = vec2f(0.0);
    var pen = 0.0;
    var gap = 0.0;
    for (var i = start; i < end; i++) {
        let a = quad(words + i * 8);
        let b = quad(words + i * 8 + 4);
        if i > start {
            pen += gap;
        }
        let adv = a.z + a.w * tracking;
        if x > pen - REACH && x < pen + adv + REACH {
            for (var j = 0; j < i32(a.y); j++) {
                // the glyph's cell from the pen and the baseline; in the atlas, and which letter
                // of the word and of the text it is
                let cell = quad(i32(a.x) + j * 8);
                let at = quad(i32(a.x) + j * 8 + 4);
                let q = (vec2f(x - pen - at.z * tracking, y) - cell.xy) / cell.zw;
                if all(q > vec2f(0.0)) && all(q < vec2f(1.0)) {
                    let texels = cell.zw * unit;
                    let t = at.xy + vec2f(0.0, top) + clamp(q * texels, vec2f(0.5), texels - 0.5);
                    let d = (0.5 - textureSampleLevel(src, smp, t / vec2f(textureDimensions(src)), 0.0).a) * 2.0 * reach;
                    let r = clamp(shown - at.w, 0.0, 1.0);
                    acc = max(acc, r * clamp(0.5 - (d - edges) / soft, vec2f(0.0), vec2f(1.0)));
                }
            }
        }
        pen += adv;
        gap = b.x + b.y * tracking;
    }
    return acc;
}

fn over(a: vec4f, b: vec4f) -> vec4f {
    return a + b * (1.0 - a.a);
}

// The atlas' layout, read once a pixel: its words in the font picked (`words`, `nwords`, every
// one of them), its ascent; the section's characters in z.
fn setup() -> vec4f {
    cols = i32(textureDimensions(src).x);
    unit = num(0);
    reach = num(1) / unit;
    top = num(3);
    // File with no file is Inter
    let f = select(0, font, font >= 0 && font * 2 < i32(num(2)));
    let section = quad((1 + f * 2 + i32(uppercase)) * 8);
    words = i32(section.x);
    nwords = i32(section.y);
    ascent = section.w;
    return section;
}

// The words from `words` (nwords of them, `shown` characters in) set and drawn at uv. audio: the
// kick pumps their size.
fn draw(uv: vec2f) -> vec4f {
    sz = size * (1.0 + 0.15 * audio * g.kick);
    limit = wrap * g.aspect / sz;
    lines = 0;
    widest = 0.0;
    measure();
    // the canvas in units, centred on position and turned (a half turn at 1)
    let a = rotate * 3.14159265;
    let turn = mat2x2f(cos(a), sin(a), -sin(a), cos(a));
    let c = (uv - 0.5 - position * 0.5) * vec2f(g.aspect, 1.0) / sz;
    let p = turn * c;
    let pixel = 1.0 / (sz * g.resolution.y);
    let bold = weight * BOLD;
    let edge = bold + outline * LINE;
    let ink = cover(p, vec2f(bold, edge), pixel);
    // the shadow falls the same way however the words turn
    var shade = 0.0;
    if shadow > 0.0 {
        shade = cover(turn * (c - shadow_offset), vec2f(edge), shadow_blur * BLUR + pixel).x * shadow;
    }
    // the box: the lines' block, padded, its corners rounded
    let half = vec2f(widest, f32(lines) * line_height) * 0.5 + box_padding;
    let r = min(box_radius, min(half.x, half.y));
    let k = abs(p) - half + r;
    let out = length(max(k, vec2f(0.0))) + min(max(k.x, k.y), 0.0) - r;
    let boxed = clamp(0.5 - out / pixel, 0.0, 1.0) * box * f32(lines > 0);
    // bottom up, premultiplied: box, shadow, outline, letters
    var o = vec4f(box_color.rgb, 1.0) * box_color.a * boxed;
    o = over(vec4f(shadow_color.rgb, 1.0) * shadow_color.a * shade, o);
    o = over(vec4f(outline_color.rgb, 1.0) * outline_color.a * ink.y * clamp(outline * LINE / pixel, 0.0, 1.0), o);
    o = over(vec4f(tint.rgb, 1.0) * tint.a * ink.x, o);
    return o * opacity;
}
