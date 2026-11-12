/*{
  "name": "Marble",
  "category": "generator",
  "description": "Polished marble, its veins slowly shifting",
  "params": [
    { "id": "scale", "type": "float", "default": 2.0, "min": 0.5, "max": 8.0 },
    { "id": "warp", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "speed", "type": "float", "default": 0.05, "min": 0.0, "max": 0.5 },
    { "id": "color_a", "type": "color", "default": [0.93, 0.91, 0.88, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.18, 0.2, 0.26, 1.0] },
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

fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2f(1.0, 0.0)), u.x), mix(hash(i + vec2f(0.0, 1.0)), hash(i + vec2f(1.0, 1.0)), u.x), u.y);
}

fn fbm(start: vec2f) -> f32 {
    var p = start;
    var sum = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 5; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let s = (uv - 0.5) * vec2f(g.aspect, 1.0);
    let p = s * scale;
    let t = g.time * speed;
    // the stone folded over itself, a little more with the bass
    let fold = vec2f(fbm(p + vec2f(t, 0.0)), fbm(p + vec2f(5.2, 1.3 - t)));
    let q = p + fold * warp * 1.5 * (1.0 + 0.3 * audio * g.bass);
    let v = sin(q.x * 2.0 + q.y + fbm(q * 2.0) * 5.0);
    let fine = sin(q.x * 5.0 - q.y * 3.0 + fbm(q * 3.0 + 7.0) * 7.0);
    let veins = pow(1.0 - abs(v), 5.0) * 0.85 + pow(1.0 - abs(fine), 18.0) * 0.35;
    var col = mix(color_a.rgb, color_b.rgb, clamp(veins, 0.0, 1.0));
    col *= 0.88 + 0.16 * fbm(p * 3.0 - 4.0);
    // the veins glow warm on the kick, glints with the hats
    col += vec3f(1.0, 0.75, 0.4) * clamp(veins, 0.0, 1.0) * 0.6 * audio * g.kick;
    col += audio * g.hat * step(0.996, hash(s * 500.0 + floor(g.time * 10.0))) * 0.8;
    return vec4f(col, 1.0);
}
