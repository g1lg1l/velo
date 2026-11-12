/*{
  "name": "Gradient ASCII",
  "category": "generator",
  "description": "Soft glows drifting in the dark, drawn in glyphs: a dot where it's dim, then a colon, a dash, a plus, a hash and a block where it's brightest; the bass swells the glows, the kick scrambles the glyphs",
  "params": [
    { "id": "size", "type": "float", "default": 10.0, "min": 4.0, "max": 40.0 },
    { "id": "weight", "type": "float", "default": 0.07, "min": 0.03, "max": 0.14 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.35, 0.2, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.55, 0.9, 1.0] },
    { "id": "background", "type": "color", "default": [0.0, 0.0, 0.02, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Gradient ASCII (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // cells a little taller than wide, `size` pixels across at 540 lines
    let cell = vec2f(1.0, 1.4) * max(floor(size * g.resolution.y / 540.0), 4.0);
    let id = floor(px / cell);
    let c = ((id + 0.5) * cell / g.resolution - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // four glows wandering on loops of their own
    var v = 0.0;
    for (var i = 0; i < 4; i++) {
        let f = f32(i);
        let d = c - vec2f(0.42 * g.aspect * sin(t * (0.5 + 0.13 * f) + f * 2.1), 0.33 * cos(t * (0.4 + 0.17 * f) + f * 1.3));
        v += exp(-dot(d, d) * (5.0 + 2.0 * f));
    }
    // a glyph flickers to its neighbor now and then; the kick scrambles one in five
    let flicker = 0.1 * (hash(id + floor(g.time * 5.0) * 7.0) - 0.5);
    let scramble = 0.5 * audio * g.kick * step(0.8, hash(id + floor(g.time * 12.0) * 13.0));
    // rolled off, so a glow's middle doesn't sit flat at the brightest glyph
    let brightness = clamp(1.0 - exp(-v * 1.3 * (1.0 + 0.5 * audio * g.bass)) + flicker + scramble, 0.0, 1.0);
    // where in the cell, mirrored into one corner: every glyph is the same four ways round
    let p = abs(fract(px / cell) - 0.5);
    let aa = 0.5 / cell.x;
    let dot_glyph = 1.0 - smoothstep(weight - aa, weight + aa, length(p));
    let colon = 1.0 - smoothstep(weight - aa, weight + aa, length(p - vec2f(0.0, 0.17)));
    let dash = (1.0 - smoothstep(weight - aa, weight + aa, p.y)) * (1.0 - smoothstep(0.24 - aa, 0.24 + aa, p.x));
    let plus = (1.0 - smoothstep(weight - aa, weight + aa, min(p.x, p.y))) * (1.0 - smoothstep(0.26 - aa, 0.26 + aa, max(p.x, p.y)));
    let bars = (1.0 - smoothstep(weight - aa, weight + aa, min(abs(p.x - 0.15), abs(p.y - 0.15)))) * (1.0 - smoothstep(0.32 - aa, 0.32 + aa, max(p.x, p.y)));
    let block = 1.0 - smoothstep(0.3 - aa, 0.3 + aa, max(p.x, p.y));
    var glyph = dot_glyph * step(0.06, brightness);
    glyph = select(glyph, colon, brightness >= 0.2);
    glyph = select(glyph, dash, brightness >= 0.35);
    glyph = select(glyph, plus, brightness >= 0.5);
    glyph = select(glyph, bars, brightness >= 0.68);
    glyph = select(glyph, block, brightness >= 0.9);
    let ink = mix(color_a.rgb, color_b.rgb, brightness) * (0.4 + 0.8 * brightness);
    return vec4f(background.rgb + ink * glyph, 1.0);
}
