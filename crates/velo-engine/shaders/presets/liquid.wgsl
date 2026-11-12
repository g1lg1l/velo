/*{
  "name": "Liquid",
  "category": "generator",
  "description": "Smooth noise folded over itself again and again, flowing like ink in water",
  "params": [
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.3, "max": 6.0 },
    { "id": "speed", "type": "float", "default": 0.1, "min": 0.0, "max": 1.0 },
    { "id": "warp", "type": "float", "default": 1.5, "min": 0.0, "max": 4.0 },
    { "id": "color_a", "type": "color", "default": [0.02, 0.04, 0.12, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.1, 0.55, 0.65, 1.0] },
    { "id": "color_c", "type": "color", "default": [0.95, 0.85, 0.6, 1.0] },
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
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale;
    let t = g.time * speed;
    let bend = warp * (1.0 + 0.3 * audio * g.bass);
    // noise moved by noise, moved by noise again
    let q = vec2f(fbm(p + vec2f(0.0, t)), fbm(p + vec2f(5.2, 1.3) - t * 0.8));
    let r = vec2f(fbm(p + bend * q + vec2f(1.7, 9.2) + t * 0.6), fbm(p + bend * q + vec2f(8.3, 2.8) - t * 0.5));
    let f = fbm(p + bend * r);
    var col = mix(color_a.rgb, color_b.rgb, clamp(f * f * 3.0, 0.0, 1.0));
    col = mix(col, color_c.rgb, clamp(smoothstep(0.45, 0.8, r.x) * f * 1.6, 0.0, 1.0));
    col *= 0.6 + 0.8 * f;
    col += color_c.rgb * audio * g.hat * 0.25 * smoothstep(0.65, 0.75, f);
    return vec4f(col * (1.0 + 0.4 * audio * g.kick), 1.0);
}
