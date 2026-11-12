/*{
  "name": "Dark Veil",
  "category": "generator",
  "description": "A silky veil of light drifting in the dark, hanging in long folds of fine threads; the bass billows it, the kick lights it",
  "params": [
    { "id": "speed", "type": "float", "default": 0.3, "min": 0.0, "max": 2.0 },
    { "id": "scale", "type": "float", "default": 1.5, "min": 0.4, "max": 6.0 },
    { "id": "threads", "type": "float", "default": 6.0, "min": 0.0, "max": 20.0 },
    { "id": "glow", "type": "float", "default": 1.0, "min": 0.0, "max": 3.0 },
    { "id": "color_a", "type": "color", "default": [0.45, 0.3, 1.0, 1.0] },
    { "id": "color_b", "type": "color", "default": [0.1, 0.8, 0.9, 1.0] },
    { "id": "background", "type": "color", "default": [0.01, 0.0, 0.03, 1.0] },
    { "id": "audio", "type": "float", "default": 0.0, "min": 0.0, "max": 2.0 }
  ]
}*/

// Velo's own, after React Bits' Dark Veil (its code isn't used: MIT + Commons Clause).

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
    for (var i = 0; i < 4; i++) {
        sum += amp * noise(p);
        p = mat2x2f(1.6, 1.2, -1.2, 1.6) * p + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum;
}

fn main_image(uv: vec2f, px: vec2f) -> vec4f {
    let p = (uv - 0.5) * vec2f(g.aspect, 1.0) * scale;
    let t = g.time * speed;
    // how the veil lies: two slow noises bend it, more with the bass
    let w = vec2f(fbm(p * 0.8 + vec2f(t * 0.2, 0.0)), fbm(p * 0.8 + vec2f(5.2, 1.3 - t * 0.15)));
    let q = p + (w - 0.5) * 2.2 * (1.0 + 0.4 * audio * g.bass);
    let fold = sin(q.x * 2.0 + q.y * 0.7 + w.x * 5.0 + t * 0.5);
    // light gathers on the crests of the folds and thins to nothing between
    let body = pow(0.5 + 0.5 * fold, 3.0);
    let fine = 0.75 + 0.25 * sin(q.x * threads * 4.0 + w.y * 20.0);
    // a band across the picture, its edges wandering
    let reach = smoothstep(1.1, 0.1, abs(q.y * 0.6 + (w.y - 0.5) * 1.2));
    let shimmer = 1.0 + 0.5 * audio * g.hat * sin(q.x * 60.0 + g.time * 25.0);
    let tint = mix(color_a.rgb, color_b.rgb, clamp(w.x * 1.6 - 0.3 + 0.3 * fold, 0.0, 1.0));
    var col = background.rgb + tint * body * fine * reach * shimmer * glow * 1.3 * (1.0 + 0.8 * audio * g.kick);
    col += mix(tint, vec3f(1.0), 0.5) * pow(body, 6.0) * reach * 0.4 * glow;
    return vec4f(col, 1.0);
}
