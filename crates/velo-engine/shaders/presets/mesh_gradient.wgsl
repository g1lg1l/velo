/*{
  "name": "Mesh Gradient",
  "category": "generator",
  "description": "Four colors melting into each other as their points drift, with a fine grain",
  "params": [
    { "id": "speed", "type": "float", "default": 0.15, "min": 0.0, "max": 1.0 },
    { "id": "warp", "type": "float", "default": 0.5, "min": 0.0, "max": 2.0 },
    { "id": "grain", "type": "float", "default": 0.04, "min": 0.0, "max": 0.3 },
    { "id": "color_a", "type": "color", "default": [0.98, 0.45, 0.35, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.35, 0.3, 0.95, 1.0] },
    { "id": "color_c", "type": "color", "default": [0.15, 0.8, 0.75, 1.0] },
    { "id": "color_d", "type": "color", "default": [1.0, 0.85, 0.5, 1.0] },
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

// how much a color's point pulls on this spot: the nearer, the more
fn pull(q: vec2f, at: vec2f) -> f32 {
    let d = q - at;
    return 1.0 / (dot(d, d) + 0.02);
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let t = g.time * speed;
    // a gentle swirl so the blends curve, swaying more with the bass
    let bend = warp * (0.12 + 0.06 * audio * g.bass);
    let q = s + bend * vec2f(sin(s.y * 3.0 + t * 1.3), cos(s.x * 3.0 - t * 1.1));
    let x = 0.5 * g.aspect;
    let wa = pull(q, vec2f(-0.55 * x, -0.25) + vec2f(sin(t * 0.7), cos(t * 0.9)) * vec2f(0.3 * x, 0.15));
    let wb = pull(q, vec2f(0.55 * x, -0.2) + vec2f(cos(t * 0.6), sin(t * 0.8)) * vec2f(0.3 * x, 0.15));
    let wc = pull(q, vec2f(-0.45 * x, 0.3) + vec2f(sin(t * 0.5 + 2.0), cos(t * 0.7 + 1.0)) * vec2f(0.3 * x, 0.12));
    let wd = pull(q, vec2f(0.5 * x, 0.25) + vec2f(cos(t * 0.8 + 3.0), sin(t * 0.6 + 2.0)) * vec2f(0.3 * x, 0.12));
    var col = (color_a.rgb * wa + color_b.rgb * wb + color_c.rgb * wc + color_d.rgb * wd) / (wa + wb + wc + wd);
    // grain, new every frame, a little rougher with the hats
    col += (hash(px + vec2f(fract(g.time * 7.31) * 4096.0, 0.0)) - 0.5) * grain * (1.0 + audio * g.hat);
    return vec4f(col * (1.0 + 0.12 * audio * g.kick), 1.0);
}
