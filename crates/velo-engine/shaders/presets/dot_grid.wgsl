/*{
  "name": "Dot Grid",
  "category": "generator",
  "description": "A grid of dots (or squares, triangles, or a mix) swelling and shrinking in waves, a ring rippling out on each kick",
  "params": [
    { "id": "density", "type": "float", "default": 24.0, "min": 4.0, "max": 100.0 },
    { "id": "size", "type": "float", "default": 0.8, "min": 0.05, "max": 1.0 },
    { "id": "speed", "type": "float", "default": 0.4, "min": 0.0, "max": 2.0 },
    { "id": "shape", "type": "int", "default": 0, "names": ["Circles", "Squares", "Triangles", "Mixed"] },
    { "id": "color_a", "type": "color", "default": [0.2, 0.85, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [1.0, 0.3, 0.6, 1.0] },
    { "id": "background", "type": "color", "default": [0.02, 0.02, 0.05, 1.0] },
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
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let q = s * density;
    let cell = floor(q);
    let f = fract(q) - 0.5;
    // the dot's center, in picture heights from the middle
    let c = (cell + 0.5) / density;
    let r0 = length(c);
    let t = g.time * speed * 4.0;
    let wave = 0.5 + 0.5 * sin(r0 * 8.0 - t + sin(c.x * 3.0 + t * 0.25) * 1.5);
    // the kick's ring moves out as the kick fades
    let ring = audio * g.kick * exp(-abs(r0 - (1.0 - g.kick) * 0.9) * 15.0);
    let r = min(size * 0.5 * ((0.3 + 0.7 * wave) * (1.0 + 0.2 * audio * g.bass) + ring), 0.5);
    let aa = density / g.resolution.y;
    // the dot's shape (Mixed: each cell its own), each about as big as the circle
    var kind = shape;
    if shape == 3 {
        kind = i32(hash(cell + 3.0) * 3.0);
    }
    var d = length(f);
    if kind == 1 {
        d = max(abs(f.x), abs(f.y)) * 1.12;
    } else if kind == 2 {
        d = max(abs(f.x) * 0.8660254 + f.y * 0.5, -f.y) * 1.6;
    }
    let spot = 1.0 - smoothstep(r - aa, r + aa, d);
    let sparkle = audio * g.hat * step(0.97, hash(cell + floor(g.time * 10.0) * 13.0));
    let tint = mix(color_a.rgb, color_b.rgb, wave) + sparkle;
    return vec4f(mix(background.rgb, tint, spot), 1.0);
}
