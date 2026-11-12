/*{
  "name": "Code Rain",
  "category": "generator",
  "description": "Columns of glyphs falling like digital rain, each led by a bright head that the kick lights up",
  "params": [
    { "id": "columns", "type": "int", "default": 48, "min": 8, "max": 160 },
    { "id": "speed", "type": "float", "default": 1.0, "min": 0.0, "max": 4.0 },
    { "id": "trail", "type": "float", "default": 12.0, "min": 2.0, "max": 40.0 },
    { "id": "density", "type": "float", "default": 0.7, "min": 0.1, "max": 1.0 },
    { "id": "tint", "type": "color", "default": [0.25, 1.0, 0.45, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// a random number 0..1 for each whole cell
fn hash(c: vec2f) -> f32 {
    let n = bitcast<vec2u>(vec2i(floor(c)));
    var h = (n.x * 1597334677u) ^ (n.y * 3812015801u);
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    // cells half again as tall as wide
    let cols = f32(columns);
    let rows = cols / (1.5 * g.aspect);
    let p = uv * vec2f(cols, rows);
    let cell = floor(p);
    let col = cell.x;
    // each column falls at its own speed from its own start; some stay dark
    let fall = 0.5 + hash(vec2f(col, 1.0));
    let head = fract(hash(vec2f(col, 2.0)) + g.time * speed * fall * 0.08) * (rows + trail) - trail * 0.2;
    let behind = head - cell.y;
    let on = step(hash(vec2f(col, 3.0)), density);
    if behind < 0.0 || behind > trail || on == 0.0 {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    // a glyph: 5 × 7 dots, its pattern changing now and then (more often with the hats)
    let f = fract(p);
    let dot_at = floor(vec2f(f.x * 6.0 - 0.5, f.y * 8.0 - 0.5));
    let inside = all(dot_at >= vec2f(0.0)) && all(dot_at < vec2f(5.0, 7.0));
    let change = floor(g.time * (2.0 + 10.0 * audio * g.hat) * (0.3 + hash(cell + 9.0)));
    let lit = select(0.0, step(0.5, hash(cell * 7.0 + dot_at + change * 13.0)), inside);
    // the head white, the trail fading behind it
    let fade = pow(1.0 - behind / trail, 1.5);
    let head_light = smoothstep(1.5, 0.0, behind) * (1.0 + audio * g.kick);
    let color = mix(tint.rgb * fade, vec3f(0.85, 1.0, 0.9), head_light);
    // a faint light behind the glyphs, so a trail reads as one streak; the bass lifts it all
    return vec4f((color * lit + tint.rgb * fade * 0.08) * (1.0 + 0.6 * audio * g.bass), 1.0);
}
