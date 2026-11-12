/*{
  "name": "Faulty Terminal",
  "category": "generator",
  "description": "A flickering old terminal full of glowing glyphs on a curved tube, rows slipping sideways now and then; the kick tears the picture, the hats make it flicker",
  "params": [
    { "id": "columns", "type": "int", "default": 64, "min": 16, "max": 160 },
    { "id": "speed", "type": "float", "default": 0.5, "min": 0.0, "max": 3.0 },
    { "id": "flicker", "type": "float", "default": 0.5, "min": 0.0, "max": 1.0 },
    { "id": "glitch", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "curvature", "type": "float", "default": 0.25, "min": 0.0, "max": 1.0 },
    { "id": "scanlines", "type": "float", "default": 0.4, "min": 0.0, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.3, 1.0, 0.5, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Faulty Terminal (its code isn't used: MIT + Commons Clause).

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x), mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // the tube's curve
    let c = uv * 2.0 - 1.0;
    let q = c * (1.0 + curvature * 0.3 * c.yx * c.yx) * 0.5 + 0.5;
    let inside = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    // cells a little taller than wide
    let cols = f32(columns);
    let rows = cols / (1.6 * g.aspect);
    var p = q * vec2f(cols, rows);
    // now and then a row slips sideways; the kick tears many at once
    let tick = floor(g.time * 8.0);
    let row = floor(p.y);
    let torn = step(1.0 - glitch * 0.12 - 0.5 * audio * g.kick, hash(vec2f(row, tick)));
    p.x += torn * (hash(vec2f(tick, row)) - 0.5) * 12.0;
    let cell = floor(p);
    let f = fract(p);
    // a glyph: 5 × 7 dots, its pattern changing now and then
    let dot_at = floor(vec2f(f.x * 6.0 - 0.5, f.y * 8.0 - 0.5));
    let in_glyph = all(dot_at >= vec2f(0.0)) && all(dot_at < vec2f(5.0, 7.0));
    let change = floor(g.time * speed * (0.5 + 5.0 * hash(cell + 9.0)));
    let lit = select(0.0, step(0.5, hash(cell * 7.0 + dot_at + change * 13.0)), in_glyph);
    // text comes and goes in drifting blocks, with blanks between words
    let field = noise(cell * vec2f(0.08, 0.15) + vec2f(0.0, -g.time * speed * 0.6));
    let bright = smoothstep(0.3, 0.7, field + 0.25 * hash(cell + floor(g.time * speed * 3.0)));
    let word = step(0.22, hash(cell + 3.0 + floor(g.time * speed * 0.7)));
    let waver = 1.0 - flicker * 0.35 * hash(vec2f(tick, 1.0)) + 0.5 * audio * g.hat * hash(vec2f(tick, 5.0));
    var col = tint.rgb * lit * word * (0.12 + 1.3 * bright) * waver;
    // the phosphor's own glow behind the glyphs, scanlines, dark corners
    col += tint.rgb * 0.05 * (0.4 + bright);
    col *= 1.0 - scanlines * (0.5 + 0.5 * sin(px.y * 3.1415927)) * 0.6;
    col *= 1.0 - 0.35 * dot(c, c);
    return vec4f(col * inside * (1.0 + 0.5 * audio * g.kick), 1.0);
}
